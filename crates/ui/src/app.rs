//! Aplicación `egui` de Raggy Desktop.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;

use raggy_core::index::{IndexEvent, IndexOptions, IndexPhase, IndexWorker};
use raggy_core::watch::FolderWatcher;
use raggy_core::{search_folder, AppConfig, AppPaths, RagConfig, APP_NAME, APP_VERSION};
use raggy_rag::{Chunk, ChunkingOptions, RetrievedChunk, RetrieverOptions};
use raggy_storage::{Document, Folder, Storage};

/// Tiempo de espera antes de reindexar tras un cambio detectado en disco.
const REINDEX_DEBOUNCE: Duration = Duration::from_millis(1200);

/// Estado de una carpeta en la interfaz.
struct FolderEntry {
    folder: Folder,
    chunks: i64,
    progress: Option<(usize, usize)>,
    message: Option<String>,
    error: Option<String>,
}

/// Acciones solicitadas desde la barra superior.
#[derive(Default)]
struct TopActions {
    add: bool,
    reindex_all: bool,
    settings: bool,
}

/// Acciones solicitadas desde el panel lateral.
#[derive(Default)]
struct SidebarActions {
    select: Option<String>,
    reindex: Option<String>,
    delete: Option<String>,
}

/// Acciones solicitadas desde el panel central.
enum CentralAction {
    Search,
    ClearSearch,
    OpenDocument(String),
}

/// Estado raíz de la interfaz.
pub struct RaggyApp {
    config: AppConfig,
    paths: AppPaths,
    storage: Option<Storage>,
    worker: Option<IndexWorker>,
    watcher: Option<FolderWatcher>,
    entries: Vec<FolderEntry>,
    selected_folder: Option<String>,
    selected_document: Option<String>,
    documents: Vec<Document>,
    chunks_preview: Vec<Chunk>,
    pending_delete: Option<String>,
    last_watch: HashMap<String, Instant>,
    query: String,
    results: Vec<RetrievedChunk>,
    search_note: Option<String>,
    settings_open: bool,
    settings_draft: RagConfig,
    status: String,
    fatal: Option<String>,
}

impl RaggyApp {
    /// Crea la aplicación, abre la base de datos y el indexador.
    pub fn new(ctx: &eframe::CreationContext<'_>, config: AppConfig, paths: AppPaths) -> Self {
        crate::theme::apply(ctx, config.theme);
        let settings_draft = config.rag.clone();

        let mut app = Self {
            config,
            paths,
            storage: None,
            worker: None,
            watcher: None,
            entries: Vec::new(),
            selected_folder: None,
            selected_document: None,
            documents: Vec::new(),
            chunks_preview: Vec::new(),
            pending_delete: None,
            last_watch: HashMap::new(),
            query: String::new(),
            results: Vec::new(),
            search_note: None,
            settings_open: false,
            settings_draft,
            status: String::new(),
            fatal: None,
        };

        match Storage::open(&app.paths.db_path()) {
            Ok(storage) => app.storage = Some(storage),
            Err(error) => {
                app.fatal = Some(format!("No se pudo abrir la base de datos: {error}"));
                return app;
            }
        }

        match IndexWorker::spawn(app.paths.db_path()) {
            Ok(worker) => app.worker = Some(worker),
            Err(error) => app.fatal = Some(format!("No se pudo iniciar el indexador: {error}")),
        }

        match FolderWatcher::new() {
            Ok(watcher) => app.watcher = Some(watcher),
            Err(error) => tracing::warn!("no se pudo iniciar el vigilante de archivos: {error}"),
        }

        app.reload_folders();
        app.sync_watcher();
        app
    }

    fn chunking_options(&self) -> ChunkingOptions {
        ChunkingOptions {
            max_chars: self.config.rag.chunk_size,
            overlap_chars: self.config.rag.chunk_overlap,
        }
    }

    fn index_options(&self) -> IndexOptions {
        IndexOptions {
            chunking: self.chunking_options(),
            embedding_dimensions: self.config.rag.embedding_dimensions,
        }
    }

    fn retriever_options(&self) -> RetrieverOptions {
        RetrieverOptions {
            top_k: self.config.rag.top_k,
            min_score: self.config.rag.similarity_threshold,
        }
    }

    // ------------------------------------------------------------- datos

    fn reload_folders(&mut self) {
        let Some(storage) = &self.storage else {
            return;
        };
        match storage.list_folders() {
            Ok(folders) => {
                self.entries = folders
                    .into_iter()
                    .map(|folder| FolderEntry {
                        chunks: storage.count_chunks(&folder.id).unwrap_or(0),
                        folder,
                        progress: None,
                        message: None,
                        error: None,
                    })
                    .collect();
            }
            Err(error) => self.status = format!("No se pudieron listar las carpetas: {error}"),
        }
    }

    fn sync_watcher(&mut self) {
        let paths: Vec<PathBuf> = self
            .entries
            .iter()
            .map(|entry| PathBuf::from(&entry.folder.path))
            .collect();
        if let Some(watcher) = self.watcher.as_mut() {
            if let Err(error) = watcher.sync(&paths) {
                tracing::warn!("no se pudo actualizar la vigilancia de carpetas: {error}");
            }
        }
    }

    fn select_folder(&mut self, id: String) {
        self.selected_folder = Some(id);
        self.selected_document = None;
        self.chunks_preview.clear();
        self.results.clear();
        self.search_note = None;
        self.reload_documents();
    }

    fn run_search(&mut self) {
        self.results.clear();
        self.search_note = None;

        let Some(folder_id) = self.selected_folder.clone() else {
            self.search_note = Some("Selecciona una carpeta para buscar.".to_string());
            return;
        };
        let query = self.query.trim().to_string();
        if query.is_empty() {
            self.search_note = Some("Escribe una consulta.".to_string());
            return;
        }

        let options = self.retriever_options();
        let dimensions = self.config.rag.embedding_dimensions;
        let Some(storage) = &self.storage else {
            return;
        };

        match search_folder(storage, &folder_id, &query, &options, dimensions) {
            Ok(results) => {
                self.search_note = Some(if results.is_empty() {
                    format!(
                        "No se encontraron fragmentos por encima del umbral ({:.2}).",
                        options.min_score
                    )
                } else {
                    format!("{} fragmentos recuperados.", results.len())
                });
                self.results = results;
            }
            Err(error) => {
                self.search_note = Some(format!("Error en la búsqueda: {error}"));
            }
        }
    }

    fn reload_documents(&mut self) {
        self.documents.clear();
        self.chunks_preview.clear();
        self.selected_document = None;

        let Some(folder_id) = self.selected_folder.clone() else {
            return;
        };
        let Some(storage) = &self.storage else {
            return;
        };
        match storage.list_documents(&folder_id) {
            Ok(documents) => self.documents = documents,
            Err(error) => self.status = format!("No se pudieron listar los archivos: {error}"),
        }
    }

    fn load_chunks(&mut self, relative_path: &str) {
        let Some(folder_id) = self.selected_folder.clone() else {
            return;
        };
        let Some(storage) = &self.storage else {
            return;
        };
        match storage.list_chunks_for_path(&folder_id, relative_path) {
            Ok(chunks) => {
                self.chunks_preview = chunks;
                self.selected_document = Some(relative_path.to_string());
            }
            Err(error) => self.status = format!("No se pudieron cargar los fragmentos: {error}"),
        }
    }

    // --------------------------------------------------------- acciones

    fn add_folder(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Selecciona una carpeta con archivos Markdown")
            .pick_folder()
        else {
            return;
        };
        self.register_folder(path);
    }

    fn register_folder(&mut self, path: PathBuf) {
        let path_str = path.to_string_lossy().to_string();

        let Some(storage) = &self.storage else {
            self.status = "La base de datos no está disponible.".to_string();
            return;
        };

        if let Ok(Some(existing)) = storage.find_folder_by_path(&path_str) {
            self.status = format!("La carpeta «{}» ya estaba indexada.", existing.name);
            self.select_folder(existing.id);
            return;
        }

        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path_str.clone());
        let folder = Folder::new(path_str, name);

        if let Err(error) = storage.upsert_folder(&folder) {
            self.status = format!("No se pudo registrar la carpeta: {error}");
            return;
        }

        self.status = format!("Indexando «{}»…", folder.name);
        self.entries.push(FolderEntry {
            folder: folder.clone(),
            chunks: 0,
            progress: Some((0, 0)),
            message: None,
            error: None,
        });
        self.select_folder(folder.id.clone());
        self.sync_watcher();
        self.start_index(&folder.id, path);
    }

    fn start_index(&mut self, folder_id: &str, root: PathBuf) {
        let options = self.index_options();
        let Some(worker) = &self.worker else {
            self.status = "El indexador no está disponible.".to_string();
            return;
        };
        worker.index_folder(folder_id.to_string(), root, options);
    }

    fn reindex_folder(&mut self, folder_id: &str) {
        let Some(entry) = self.entries.iter().find(|e| e.folder.id == folder_id) else {
            return;
        };
        let root = PathBuf::from(&entry.folder.path);
        self.status = format!("Reindexando «{}»…", entry.folder.name);
        self.start_index(folder_id, root);
    }

    fn reindex_all(&mut self) {
        let options = self.index_options();
        let Some(worker) = &self.worker else {
            self.status = "El indexador no está disponible.".to_string();
            return;
        };
        worker.reindex_all(options);
        self.status = "Reindexando todas las carpetas…".to_string();
    }

    fn delete_folder(&mut self, id: &str) {
        if let Some(storage) = &self.storage {
            if let Err(error) = storage.delete_folder(id) {
                self.status = format!("No se pudo eliminar la carpeta: {error}");
                return;
            }
        }
        self.entries.retain(|entry| entry.folder.id != id);
        if self.selected_folder.as_deref() == Some(id) {
            self.selected_folder = None;
            self.reload_documents();
        }
        self.last_watch.remove(id);
        self.sync_watcher();
        self.status = "Carpeta eliminada del índice.".to_string();
    }

    // ------------------------------------------------------------ eventos

    fn drain_events(&mut self) {
        let Some(worker) = &self.worker else {
            return;
        };
        let events = worker.drain();
        for event in events {
            match event {
                IndexEvent::Progress(progress) => {
                    if let Some(entry) = self
                        .entries
                        .iter_mut()
                        .find(|entry| entry.folder.id == progress.folder_id)
                    {
                        entry.progress = match progress.phase {
                            IndexPhase::Done | IndexPhase::Failed => None,
                            _ => Some((progress.done, progress.total)),
                        };
                    }
                }
                IndexEvent::Finished(summary) => {
                    if let Some(entry) = self
                        .entries
                        .iter_mut()
                        .find(|entry| entry.folder.id == summary.folder_id)
                    {
                        entry.progress = None;
                        entry.error = None;
                        entry.message = Some(format!(
                            "{} indexados · {} sin cambios · {} retirados · {} fragmentos",
                            summary.files_indexed,
                            summary.files_skipped,
                            summary.files_removed,
                            summary.chunks_written
                        ));
                    }
                    self.status = format!(
                        "Indexación completada: {} archivos actualizados.",
                        summary.files_indexed
                    );
                    self.results.clear();
                    self.search_note = None;
                    self.refresh_for(&summary.folder_id);
                }
                IndexEvent::Failed { folder_id, message } => {
                    if let Some(entry) = self
                        .entries
                        .iter_mut()
                        .find(|entry| entry.folder.id == folder_id)
                    {
                        entry.progress = None;
                        entry.error = Some(message.clone());
                    }
                    self.status = format!("Indexación fallida: {message}");
                }
            }
        }
    }

    fn refresh_for(&mut self, folder_id: &str) {
        if let Some(storage) = &self.storage {
            if let Ok(count) = storage.count_chunks(folder_id) {
                if let Some(entry) = self
                    .entries
                    .iter_mut()
                    .find(|entry| entry.folder.id == folder_id)
                {
                    entry.chunks = count;
                }
            }
        }
        if self.selected_folder.as_deref() == Some(folder_id) {
            self.reload_documents();
        }
    }

    fn poll_watcher(&mut self) {
        let Some(watcher) = &self.watcher else {
            return;
        };
        let changed = watcher.poll();
        if changed.is_empty() {
            return;
        }

        let now = Instant::now();
        let mut candidates = Vec::new();
        for entry in &self.entries {
            let root = PathBuf::from(&entry.folder.path);
            if changed.iter().any(|path| path.starts_with(&root)) {
                let due = self
                    .last_watch
                    .get(&entry.folder.id)
                    .is_none_or(|instant| now.duration_since(*instant) > REINDEX_DEBOUNCE);
                if due {
                    candidates.push((entry.folder.id.clone(), root));
                }
            }
        }

        for (id, root) in candidates {
            self.last_watch.insert(id.clone(), now);
            self.start_index(&id, root);
        }
    }

    // ------------------------------------------------------------ interfaz

    fn top_bar(ui: &mut egui::Ui, data_dir: &Path) -> TopActions {
        let mut actions = TopActions::default();
        egui::Panel::top("top_bar").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading(APP_NAME);
                ui.separator();
                ui.label(format!("v{APP_VERSION} · Fase 2"));
                ui.separator();
                if ui.button("Añadir carpeta").clicked() {
                    actions.add = true;
                }
                if ui.button("Reindexar todo").clicked() {
                    actions.reindex_all = true;
                }
                if ui.button("Configuración").clicked() {
                    actions.settings = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("Datos: {}", data_dir.display()));
                });
            });
            ui.add_space(4.0);
        });
        actions
    }

    fn sidebar(
        ui: &mut egui::Ui,
        entries: &[FolderEntry],
        selected: Option<&str>,
    ) -> SidebarActions {
        let mut actions = SidebarActions::default();
        egui::Panel::left("sidebar")
            .default_size(300.0)
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.strong("Carpetas indexadas");
                ui.add_space(6.0);

                if entries.is_empty() {
                    ui.label("Aún no hay carpetas. Usa «Añadir carpeta».");
                    return;
                }

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for entry in entries {
                        let is_selected = selected == Some(entry.folder.id.as_str());
                        if ui
                            .selectable_label(is_selected, &entry.folder.name)
                            .clicked()
                        {
                            actions.select = Some(entry.folder.id.clone());
                        }

                        let status = match (&entry.progress, &entry.error) {
                            (Some((done, total)), _) => format!("Indexando… {done}/{total}"),
                            (None, Some(error)) => format!("Error: {error}"),
                            (None, None) => format!(
                                "{} archivos · {} fragmentos",
                                entry.folder.file_count, entry.chunks
                            ),
                        };
                        ui.small(status);

                        if let Some(message) = &entry.message {
                            ui.small(message);
                        }

                        ui.horizontal(|ui| {
                            if ui.small_button("Reindexar").clicked() {
                                actions.reindex = Some(entry.folder.id.clone());
                            }
                            if ui.small_button("Eliminar").clicked() {
                                actions.delete = Some(entry.folder.id.clone());
                            }
                        });
                        ui.add_space(8.0);
                    }
                });
            });
        actions
    }

    fn status_bar(ui: &mut egui::Ui, status: &str) {
        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.small(if status.is_empty() { "Listo." } else { status });
            });
            ui.add_space(2.0);
        });
    }

    /// Dibuja el panel central y devuelve las acciones solicitadas.
    fn central(&mut self, ui: &mut egui::Ui) -> Vec<CentralAction> {
        let mut actions = Vec::new();
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(fatal) = &self.fatal {
                ui.colored_label(egui::Color32::RED, fatal);
                return;
            }

            let Some(folder_id) = self.selected_folder.clone() else {
                ui.vertical_centered(|ui| {
                    ui.add_space(96.0);
                    ui.heading(APP_NAME);
                    ui.add_space(8.0);
                    ui.label("Añade una carpeta con archivos Markdown para indexarla.");
                    ui.add_space(16.0);
                    ui.label(
                        "Después podrás buscar en su contenido y ver los fragmentos relevantes.",
                    );
                });
                return;
            };

            if let Some(entry) = self
                .entries
                .iter()
                .find(|entry| entry.folder.id == folder_id)
            {
                ui.add_space(6.0);
                ui.heading(&entry.folder.name);
                ui.small(&entry.folder.path);
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Buscar en esta carpeta…")
                        .desired_width(420.0),
                );
                let submitted = response.lost_focus()
                    && ui.ctx().input(|input| input.key_pressed(egui::Key::Enter));
                if ui.button("Buscar").clicked() || submitted {
                    actions.push(CentralAction::Search);
                }
                if !self.results.is_empty() && ui.button("Limpiar").clicked() {
                    actions.push(CentralAction::ClearSearch);
                }
            });

            if let Some(note) = &self.search_note {
                ui.small(note);
            }
            ui.separator();

            if !self.results.is_empty() {
                egui::ScrollArea::vertical()
                    .id_salt("search_results")
                    .show(ui, |ui| {
                        for (index, result) in self.results.iter().enumerate() {
                            let metadata = &result.chunk.metadata;
                            let heading =
                                metadata.heading.clone().unwrap_or_else(|| "—".to_string());
                            let citation = format!(
                                "{}:{}-{}",
                                metadata.source_path, metadata.start_line, metadata.end_line
                            );
                            let label = format!(
                                "{}. [{:.2}] {}  ·  {}",
                                index + 1,
                                result.score,
                                citation,
                                heading
                            );
                            if ui.selectable_label(false, label).clicked() {
                                actions.push(CentralAction::OpenDocument(
                                    metadata.source_path.clone(),
                                ));
                            }
                            ui.small(snippet(&result.chunk.content));
                            ui.add_space(6.0);
                        }
                    });
                return;
            }

            if self.documents.is_empty() {
                ui.label("Sin archivos indexados en esta carpeta.");
                return;
            }

            ui.label(format!("{} archivos indexados", self.documents.len()));
            ui.add_space(6.0);

            egui::ScrollArea::vertical()
                .id_salt("documents")
                .show(ui, |ui| {
                    egui::Grid::new("documents_grid")
                        .num_columns(4)
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong("Archivo");
                            ui.strong("Fragmentos");
                            ui.strong("Bytes");
                            ui.strong("Indexado");
                            ui.end_row();

                            for document in &self.documents {
                                let is_selected = self.selected_document.as_deref()
                                    == Some(&document.relative_path);
                                if ui
                                    .selectable_label(is_selected, &document.relative_path)
                                    .clicked()
                                {
                                    actions.push(CentralAction::OpenDocument(
                                        document.relative_path.clone(),
                                    ));
                                }
                                ui.label(document.chunk_count.to_string());
                                ui.label(document.size_bytes.to_string());
                                ui.label(short_timestamp(&document.indexed_at));
                                ui.end_row();
                            }
                        });
                });
        });
        actions
    }

    /// Devuelve las líneas que se están indexando en disco.
    fn chunks_panel(&self, ui: &mut egui::Ui) {
        let Some(relative_path) = self.selected_document.clone() else {
            return;
        };

        egui::Panel::right("chunks_panel")
            .default_size(460.0)
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.strong(relative_path);
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.chunks_preview.is_empty() {
                        ui.label("Sin fragmentos.");
                        return;
                    }
                    for (index, chunk) in self.chunks_preview.iter().enumerate() {
                        ui.label(format!(
                            "Fragmento {} · líneas {}-{}",
                            index + 1,
                            chunk.metadata.start_line,
                            chunk.metadata.end_line
                        ));
                        if let Some(heading) = &chunk.metadata.heading {
                            ui.small(format!("Encabezado: {heading}"));
                        }
                        ui.add(
                            egui::Label::new(egui::RichText::new(&chunk.content).monospace())
                                .wrap(),
                        );
                        ui.separator();
                    }
                });
            });
    }

    fn delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self.pending_delete.clone() else {
            return;
        };
        let name = self
            .entries
            .iter()
            .find(|entry| entry.folder.id == id)
            .map(|entry| entry.folder.name.clone())
            .unwrap_or_default();

        let mut confirm = false;
        let mut cancel = false;

        egui::Window::new("Eliminar carpeta")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!("¿Eliminar «{name}» del índice?"));
                ui.small(
                    "Se borrarán sus fragmentos y metadatos. Los archivos en disco no se tocan.",
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancelar").clicked() {
                        cancel = true;
                    }
                    if ui.button("Eliminar").clicked() {
                        confirm = true;
                    }
                });
            });

        if confirm {
            self.pending_delete = None;
            self.delete_folder(&id);
        } else if cancel {
            self.pending_delete = None;
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }

        let mut open = true;
        let mut save = false;
        let mut cancel = false;

        egui::Window::new("Configuración del RAG")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                let draft = &mut self.settings_draft;
                egui::Grid::new("settings_grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Tamaño de fragmento (caracteres)");
                        ui.add(egui::DragValue::new(&mut draft.chunk_size).range(100..=4000));
                        ui.end_row();

                        ui.label("Solapamiento (caracteres)");
                        ui.add(egui::DragValue::new(&mut draft.chunk_overlap).range(0..=2000));
                        ui.end_row();

                        ui.label("Fragmentos recuperados (top-k)");
                        ui.add(egui::DragValue::new(&mut draft.top_k).range(1..=50));
                        ui.end_row();

                        ui.label("Umbral de similitud");
                        ui.add(
                            egui::DragValue::new(&mut draft.similarity_threshold)
                                .range(0.0..=1.0)
                                .speed(0.01),
                        );
                        ui.end_row();

                        ui.label("Dimensionalidad del embedding");
                        ui.add(
                            egui::DragValue::new(&mut draft.embedding_dimensions).range(16..=4096),
                        );
                        ui.end_row();
                    });

                ui.add_space(6.0);
                ui.small("Cambiar el tamaño de fragmento o la dimensionalidad requiere reindexar.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Guardar").clicked() {
                        save = true;
                    }
                    if ui.button("Cancelar").clicked() {
                        cancel = true;
                    }
                });
            });

        if save {
            let mut candidate = self.config.clone();
            candidate.rag = self.settings_draft.clone();
            match candidate.validate() {
                Ok(()) => {
                    self.config = candidate;
                    match self.config.save(&self.paths.config_path()) {
                        Ok(()) => {
                            self.status = "Configuración guardada. Reindexa para aplicar cambios \
                                 de fragmentación o embeddings."
                                .to_string();
                            self.settings_open = false;
                        }
                        Err(error) => {
                            self.status = format!("No se pudo guardar la configuración: {error}");
                        }
                    }
                }
                Err(error) => {
                    self.status = format!("Configuración inválida: {error}");
                }
            }
        } else if cancel || !open {
            self.settings_draft = self.config.rag.clone();
            self.settings_open = false;
        }
    }

    fn handle_actions(&mut self, ui: &mut egui::Ui) {
        let actions = Self::top_bar(ui, &self.paths.data_dir);
        if actions.add {
            self.add_folder();
        }
        if actions.reindex_all {
            self.reindex_all();
        }
        if actions.settings {
            self.settings_draft = self.config.rag.clone();
            self.settings_open = true;
        }

        let sidebar = Self::sidebar(ui, &self.entries, self.selected_folder.as_deref());
        if let Some(id) = sidebar.select {
            self.select_folder(id);
        }
        if let Some(id) = sidebar.reindex {
            self.reindex_folder(&id);
        }
        if let Some(id) = sidebar.delete {
            self.pending_delete = Some(id);
        }

        // Los paneles deben declararse antes que el panel central.
        self.chunks_panel(ui);
        Self::status_bar(ui, &self.status);
        for action in self.central(ui) {
            match action {
                CentralAction::Search => self.run_search(),
                CentralAction::ClearSearch => {
                    self.results.clear();
                    self.search_note = None;
                }
                CentralAction::OpenDocument(path) => self.load_chunks(&path),
            }
        }
    }
}

impl eframe::App for RaggyApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        self.drain_events();
        self.poll_watcher();
        if self.worker.is_some() || self.watcher.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }

        self.handle_actions(ui);
        self.delete_dialog(&ctx);
        self.settings_window(&ctx);
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        if let Err(error) = self.config.save(&self.paths.config_path()) {
            tracing::warn!("no se pudo guardar la configuración al cerrar: {error}");
        }
    }
}

/// Abrevia una marca de tiempo RFC 3339 a fecha y hora.
fn short_timestamp(value: &str) -> String {
    value
        .get(..16)
        .map(|slice| slice.replace('T', " "))
        .unwrap_or_else(|| value.to_string())
}

/// Extrae la primera línea con contenido de un fragmento, recortada.
fn snippet(content: &str) -> String {
    let line = content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let mut snippet: String = line.chars().take(140).collect();
    if line.chars().count() > 140 {
        snippet.push('…');
    }
    snippet
}

#[cfg(test)]
mod tests {
    use super::{short_timestamp, snippet};

    #[test]
    fn abrevia_marcas_de_tiempo() {
        assert_eq!(
            short_timestamp("2026-10-14T02:15:27.459528Z"),
            "2026-10-14 02:15"
        );
        assert_eq!(short_timestamp("corto"), "corto");
    }

    #[test]
    fn recorta_el_extracto_a_la_primera_linea() {
        assert_eq!(snippet("\n\n  hola mundo  \nresto"), "hola mundo");
        assert_eq!(snippet("   "), "");
    }
}
