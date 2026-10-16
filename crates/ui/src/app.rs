//! Aplicación `egui` de Raggy Desktop.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use zeroize::Zeroize;

use raggy_core::chat::{compose_request, context_from_retrieved};
use raggy_core::index::{IndexEvent, IndexOptions, IndexPhase, IndexWorker};
use raggy_core::watch::FolderWatcher;
use raggy_core::{
    artifacts_for_prompt, relative_to, resolve_key, save_artifact, tools_report, AppConfig,
    AppPaths, ChatEvent, ChatWorker, ConflictPolicy, ExtractOptions, IngestConfig, ProviderFactory,
    RagConfig, SaveOutcome, SearchCache, SecretStore, SettingsStore, Theme, APP_NAME, APP_VERSION,
};
use raggy_llm::{extract_code_blocks, suggest_filename, CodeBlock, ProviderId};
use raggy_rag::{
    verify_citations, Chunk, ChunkingOptions, Citation, RetrievedChunk, RetrieverOptions,
};
use raggy_storage::{Artifact, Document, Folder, Message, Session, Storage};

/// Tiempo de espera antes de reindexar tras un cambio detectado en disco.
const REINDEX_DEBOUNCE: Duration = Duration::from_millis(1200);

/// Altura uniforme de los controles de la cabecera de la sesión de chat.
///
/// Al fijarla antes de crear la fila, el título, los desplegables, los
/// separadores y los botones comparten la misma altura y quedan alineados.
const SESSION_HEADER_HEIGHT: f32 = 32.0;

/// Altura de la caja de texto y del botón del compositor del chat.
const COMPOSER_INPUT_HEIGHT: f32 = 76.0;

/// Estado de una clave de API en la interfaz (HU-23, HU-24).
#[derive(Clone)]
enum KeyStatus {
    /// No hay clave configurada.
    Missing,
    /// Clave guardada en `settings.json` (enmascarada).
    Stored { masked: String },
    /// Clave tomada de una variable de entorno.
    Environment { env_var: &'static str },
    /// Comprobación de conexión en curso.
    Testing,
    /// Conexión correcta con los modelos disponibles.
    Ok { count: usize, sample: String },
    /// Error de conexión o del almacén.
    Error(String),
}

/// Acciones del panel de claves.
enum KeyAction {
    Save(ProviderId),
    Delete(ProviderId),
    Test(ProviderId),
}

/// Fallo de generación recordado para ofrecer el respaldo (HU-25).
struct Failure {
    session_id: String,
    availability: bool,
}

/// Descarga individual pendiente de resolver un conflicto de nombre (HU-17).
struct PendingDownload {
    name: String,
    language: Option<String>,
    content: String,
    root: PathBuf,
}

/// Elemento de una descarga por lote (HU-19).
struct BatchItem {
    relative: String,
    language: Option<String>,
    content: String,
}

/// Estado del diálogo de descarga por lote (HU-19).
struct BatchDownload {
    session_id: String,
    root: PathBuf,
    items: Vec<BatchItem>,
    overwrite: bool,
}

/// Ámbito de la vista de artefactos (HU-20).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArtifactScope {
    Session,
    Folder,
}

/// Estado de una carpeta en la interfaz.
struct FolderEntry {
    folder: Folder,
    chunks: i64,
    progress: Option<(usize, usize)>,
    message: Option<String>,
    error: Option<String>,
}

/// Generación en curso.
struct StreamingState {
    session_id: String,
    text: String,
}

/// Datos de un mensaje ya calculados para dibujarlo, de modo que el bucle de la
/// interfaz no vuelva a partir el texto ni a verificar citas en cada fotograma.
struct RenderedMessage {
    prose: String,
    blocks: Vec<CodeBlock>,
    citations: Vec<Citation>,
}

/// Calcula la prosa, los bloques de código y las citas verificadas de un mensaje.
fn render_message(message: &Message) -> RenderedMessage {
    if message.role == "user" {
        RenderedMessage {
            prose: message.content.clone(),
            blocks: Vec::new(),
            citations: Vec::new(),
        }
    } else {
        RenderedMessage {
            prose: raggy_llm::strip_code_blocks(&message.content),
            blocks: extract_code_blocks(&message.content),
            citations: verify_citations(&message.content, &message.citations),
        }
    }
}

/// Diálogo modal de sesiones.
enum SessionDialog {
    Rename { id: String, title: String },
    Delete { id: String },
}

/// Acciones solicitadas desde la barra superior.
#[derive(Default)]
struct TopActions {
    add: bool,
    upload: bool,
    reindex_all: bool,
    settings: bool,
    keys: bool,
    artifacts: bool,
}

/// Acciones de la vista de artefactos.
enum ArtifactAction {
    Open(PathBuf),
    Archive(String, bool),
    Delete(String),
}

/// Acciones solicitadas desde el panel lateral.
#[derive(Default)]
struct SidebarActions {
    select_folder: Option<String>,
    reindex: Option<String>,
    delete_folder: Option<String>,
    new_session: bool,
    select_session: Option<String>,
    rename_session: Option<String>,
    delete_session: Option<String>,
}

/// Acciones solicitadas desde el panel central.
enum CentralAction {
    Search,
    ClearSearch,
    OpenDocument(String),
    OpenCitation(Citation),
    SendMessage,
    StopGeneration,
    UpdateModel {
        provider: ProviderId,
        model: String,
    },
    UpdateFallback {
        provider: Option<ProviderId>,
    },
    RetryWithFallback,
    AttachFiles,
    RemoveAttachment(String),
    CopyCode(String),
    DownloadBlock {
        message_id: String,
        block_index: usize,
    },
    DownloadAll {
        message_id: String,
    },
    ExportSession,
}

/// Estado raíz de la interfaz.
pub struct RaggyApp {
    config: AppConfig,
    paths: AppPaths,
    storage: Option<Storage>,
    worker: Option<IndexWorker>,
    chat_worker: Option<ChatWorker>,
    watcher: Option<FolderWatcher>,
    entries: Vec<FolderEntry>,
    selected_folder: Option<String>,
    selected_document: Option<String>,
    selected_chunk_id: Option<String>,
    documents: Vec<Document>,
    chunks_preview: Vec<Chunk>,
    /// ¿Está abierto el panel lateral de fragmentos? (se cierra con la ✕ o Esc)
    chunks_panel_open: bool,
    /// Título del panel: la ruta del archivo o la etiqueta de la cita.
    chunks_panel_title: String,
    pending_delete: Option<String>,
    last_watch: HashMap<String, Instant>,
    query: String,
    results: Vec<RetrievedChunk>,
    search_cache: SearchCache,
    search_note: Option<String>,
    sessions: Vec<Session>,
    active_session: Option<Session>,
    attachments: Vec<Document>,
    messages: Vec<Message>,
    rendered: HashMap<String, RenderedMessage>,
    chat_input: String,
    streaming: Option<StreamingState>,
    chat_error: Option<String>,
    session_dialog: Option<SessionDialog>,
    settings_open: bool,
    settings_draft: RagConfig,
    settings_ingest: IngestConfig,
    settings_languages: String,
    tools_report: Option<String>,
    index_warnings: Vec<String>,
    visuals_applied: Option<(Theme, bool)>,
    secrets: Arc<dyn SecretStore>,
    keys_open: bool,
    key_tab: ProviderId,
    key_inputs: HashMap<ProviderId, String>,
    key_status: HashMap<ProviderId, KeyStatus>,
    test_tx: Sender<(ProviderId, Result<Vec<String>, String>)>,
    test_rx: Receiver<(ProviderId, Result<Vec<String>, String>)>,
    /// Modelos que el proveedor ha detectado con su API Key (HU-24).
    detected_models: HashMap<ProviderId, Vec<String>>,
    /// Proveedores con una detección de modelos en curso.
    detecting_models: HashSet<ProviderId>,
    /// Último error al detectar modelos, por proveedor.
    model_errors: HashMap<ProviderId, String>,
    detect_tx: Sender<(ProviderId, Result<Vec<String>, String>)>,
    detect_rx: Receiver<(ProviderId, Result<Vec<String>, String>)>,
    /// Proveedor cuyo aviso flotante de API Key ausente está abierto (HU-23).
    api_key_alert: Option<ProviderId>,
    last_failure: Option<Failure>,
    artifacts: Vec<Artifact>,
    artifacts_open: bool,
    artifact_scope: ArtifactScope,
    code_edits: HashMap<String, String>,
    previews: HashMap<String, bool>,
    pending_download: Option<PendingDownload>,
    batch: Option<BatchDownload>,
    last_download_dir: Option<PathBuf>,
    data_dir_draft: Option<PathBuf>,
    pending_import: Option<PathBuf>,
    status: String,
    fatal: Option<String>,
}

impl RaggyApp {
    /// Crea la aplicación, abre la base de datos, el indexador y el chat.
    pub fn new(ctx: &eframe::CreationContext<'_>, config: AppConfig, paths: AppPaths) -> Self {
        crate::theme::apply(ctx, config.theme);
        egui_extras::install_image_loaders(&ctx.egui_ctx);
        let settings_draft = config.rag.clone();
        let settings_ingest = config.ingest.clone();
        let settings_languages = config.ingest.ocr_languages.join(", ");
        let secrets: Arc<dyn SecretStore> = Arc::new(SettingsStore::new(paths.settings_path()));
        let key_tab =
            ProviderId::from_key(&config.model.default_provider).unwrap_or(ProviderId::DeepSeek);
        let (test_tx, test_rx) = mpsc::channel();
        let (detect_tx, detect_rx) = mpsc::channel();

        let mut app = Self {
            config,
            paths,
            storage: None,
            worker: None,
            chat_worker: None,
            watcher: None,
            entries: Vec::new(),
            selected_folder: None,
            selected_document: None,
            selected_chunk_id: None,
            documents: Vec::new(),
            chunks_preview: Vec::new(),
            chunks_panel_open: false,
            chunks_panel_title: String::new(),
            pending_delete: None,
            last_watch: HashMap::new(),
            query: String::new(),
            results: Vec::new(),
            search_cache: SearchCache::default(),
            search_note: None,
            sessions: Vec::new(),
            active_session: None,
            attachments: Vec::new(),
            messages: Vec::new(),
            rendered: HashMap::new(),
            chat_input: String::new(),
            streaming: None,
            chat_error: None,
            session_dialog: None,
            settings_open: false,
            settings_draft,
            settings_ingest,
            settings_languages,
            tools_report: None,
            index_warnings: Vec::new(),
            visuals_applied: None,
            secrets,
            keys_open: false,
            key_tab,
            key_inputs: HashMap::new(),
            key_status: HashMap::new(),
            test_tx,
            test_rx,
            detected_models: HashMap::new(),
            detecting_models: HashSet::new(),
            model_errors: HashMap::new(),
            detect_tx,
            detect_rx,
            api_key_alert: None,
            last_failure: None,
            artifacts: Vec::new(),
            artifacts_open: false,
            artifact_scope: ArtifactScope::Session,
            code_edits: HashMap::new(),
            previews: HashMap::new(),
            pending_download: None,
            batch: None,
            last_download_dir: None,
            data_dir_draft: None,
            pending_import: None,
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

        let factory_secrets = Arc::clone(&app.secrets);
        let factory: ProviderFactory =
            Arc::new(move |id| match resolve_key(factory_secrets.as_ref(), id) {
                Some(secret) => raggy_llm::from_api_key(id, secret.expose().to_string()),
                None => Err(raggy_llm::ProviderError::MissingApiKey {
                    provider: id.display_name(),
                    env_var: id.api_key_env(),
                }),
            });
        match ChatWorker::spawn(app.paths.db_path(), factory) {
            Ok(worker) => app.chat_worker = Some(worker),
            Err(error) => app.fatal = Some(format!("No se pudo iniciar el chat: {error}")),
        }

        match FolderWatcher::new() {
            Ok(watcher) => app.watcher = Some(watcher),
            Err(error) => tracing::warn!("no se pudo iniciar el vigilante de archivos: {error}"),
        }

        app.reload_folders();
        app.sync_watcher();
        app.refresh_keys();

        // Si el índice se hizo con otro pipeline (otra versión de la app, otras
        // opciones de ingesta o herramientas recién instaladas), se reindexa solo;
        // vale para las carpetas y para los adjuntos de los chats.
        if let Some(worker) = &app.worker {
            worker.reindex_stale(app.index_options(), app.paths.attachments_dir());
        }

        app
    }

    // --------------------------------------------------------------- claves

    /// Recalcula el estado de las claves de cada proveedor (HU-23).
    fn refresh_keys(&mut self) {
        for provider in ProviderId::ALL {
            let status = match self.secrets.get(provider) {
                Ok(Some(mut secret)) => {
                    let masked = raggy_core::secrets::mask(&secret);
                    secret.zeroize();
                    KeyStatus::Stored { masked }
                }
                Ok(None) => {
                    if raggy_llm::api_key(provider).is_some() {
                        KeyStatus::Environment {
                            env_var: provider.api_key_env(),
                        }
                    } else {
                        KeyStatus::Missing
                    }
                }
                Err(error) => KeyStatus::Error(error.to_string()),
            };
            let configured = matches!(
                status,
                KeyStatus::Stored { .. } | KeyStatus::Environment { .. } | KeyStatus::Ok { .. }
            );
            self.key_status.insert(provider, status);

            // Si hay clave y aún no se conocen sus modelos, se detectan al vuelo
            // para que el selector no muestre una lista fija (HU-24).
            if configured
                && !self.detected_models.contains_key(&provider)
                && !self.detecting_models.contains(&provider)
            {
                self.detect_models(provider);
            }
        }
    }

    /// Borra las copias de claves que mantiene la interfaz antes de salir.
    fn wipe_secret_copies(&mut self) {
        for secret in self.config.api_keys.values_mut() {
            secret.zeroize();
        }
        for input in self.key_inputs.values_mut() {
            input.zeroize();
        }
        self.key_inputs.clear();
    }

    fn save_key(&mut self, provider: ProviderId) {
        let secret = self
            .key_inputs
            .get(&provider)
            .map(|value| value.trim().to_string())
            .unwrap_or_default();

        if secret.is_empty() {
            self.key_status.insert(
                provider,
                KeyStatus::Error("introduce una clave antes de guardar".to_string()),
            );
            return;
        }

        let secret = raggy_core::Secret::new(secret);
        match self.secrets.set(provider, secret.expose()) {
            Ok(()) => {
                if let Some(previous) = self.config.api_keys.get_mut(provider.key()) {
                    previous.zeroize();
                }
                self.config
                    .api_keys
                    .insert(provider.key().to_string(), secret.expose().to_string());
                if let Some(mut input) = self.key_inputs.remove(&provider) {
                    input.zeroize();
                }
                self.refresh_keys();
                // La clave cambió: se descarta la lista anterior y se detecta de nuevo.
                self.detected_models.remove(&provider);
                self.detect_models(provider);
                self.status = format!(
                    "Clave de {} guardada en settings.json.",
                    provider.display_name()
                );
            }
            Err(error) => {
                self.key_status
                    .insert(provider, KeyStatus::Error(error.to_string()));
            }
        }
    }

    fn delete_key(&mut self, provider: ProviderId) {
        match self.secrets.delete(provider) {
            Ok(()) => {
                if let Some(mut previous) = self.config.api_keys.remove(provider.key()) {
                    previous.zeroize();
                }
                self.refresh_keys();
                self.detected_models.remove(&provider);
                self.model_errors.remove(&provider);
                self.status = format!("Clave de {} eliminada.", provider.display_name());
            }
            Err(error) => {
                self.key_status
                    .insert(provider, KeyStatus::Error(error.to_string()));
            }
        }
    }

    fn test_key(&mut self, provider: ProviderId) {
        let Some(secret) = resolve_key(self.secrets.as_ref(), provider) else {
            self.key_status.insert(
                provider,
                KeyStatus::Error(format!(
                    "no hay clave para {} (define {})",
                    provider.display_name(),
                    provider.api_key_env()
                )),
            );
            return;
        };

        self.key_status.insert(provider, KeyStatus::Testing);
        let sender = self.test_tx.clone();
        std::thread::spawn(move || {
            let result = raggy_llm::check_connection_with_key(provider, secret.expose())
                .map_err(|error| error.to_string());
            let _ = sender.send((provider, result));
        });
    }

    fn drain_key_tests(&mut self) {
        for (provider, result) in self.test_rx.try_iter() {
            let status = match result {
                Ok(models) => {
                    // La prueba de conexión ya trae los modelos: sirve también
                    // para el selector (HU-24).
                    if !models.is_empty() {
                        self.detected_models.insert(provider, models.clone());
                        self.model_errors.remove(&provider);
                    }
                    KeyStatus::Ok {
                        count: models.len(),
                        sample: models.first().cloned().unwrap_or_default(),
                    }
                }
                Err(message) => KeyStatus::Error(message),
            };
            self.key_status.insert(provider, status);
        }
    }

    /// Lanza en segundo plano la detección de los modelos del proveedor (HU-24).
    ///
    /// Solo se consulta si hay una clave; así el selector de modelo ofrece lo que
    /// el proveedor devuelve de verdad y no una lista fija.
    fn detect_models(&mut self, provider: ProviderId) {
        if self.detecting_models.contains(&provider) {
            return;
        }
        let Some(secret) = resolve_key(self.secrets.as_ref(), provider) else {
            return;
        };

        self.detecting_models.insert(provider);
        self.model_errors.remove(&provider);
        let sender = self.detect_tx.clone();
        std::thread::spawn(move || {
            let result = raggy_llm::check_connection_with_key(provider, secret.expose())
                .map_err(|error| error.to_string());
            let _ = sender.send((provider, result));
        });
    }

    fn drain_model_detections(&mut self) {
        for (provider, result) in self.detect_rx.try_iter() {
            self.detecting_models.remove(&provider);
            match result {
                Ok(models) if !models.is_empty() => {
                    self.detected_models.insert(provider, models);
                    self.model_errors.remove(&provider);
                }
                Ok(_) => {
                    self.model_errors.insert(
                        provider,
                        "el proveedor no devolvió ningún modelo".to_string(),
                    );
                }
                Err(error) => {
                    self.model_errors.insert(provider, error);
                }
            }
        }
    }

    /// Abre el aviso flotante de API Key ausente para un proveedor.
    fn ask_api_key(&mut self, provider: ProviderId) {
        self.api_key_alert = Some(provider);
    }

    // ---------------------------------------------------------- configuración

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
            extract: ExtractOptions {
                ocr_enabled: self.config.ingest.ocr_enabled,
                ocr_languages: self.config.ingest.ocr_languages.clone(),
                min_text_chars: self.config.ingest.min_text_chars,
                max_file_bytes: self.config.ingest.max_file_mb
                    * raggy_core::ingest_limits::BYTES_PER_MIB,
                max_archive_bytes: self.config.ingest.max_archive_mb
                    * raggy_core::ingest_limits::BYTES_PER_MIB,
                max_pages: self.config.ingest.max_pages,
                max_pdf_page_text_bytes: self.config.ingest.max_pdf_page_text_mb
                    * raggy_core::ingest_limits::BYTES_PER_MIB,
                max_pdf_text_bytes: self.config.ingest.max_pdf_text_mb
                    * raggy_core::ingest_limits::BYTES_PER_MIB,
                process_timeout_secs: self.config.ingest.process_timeout_secs,
                max_ocr_image_pixels: self.config.ingest.max_ocr_image_pixels,
            },
        }
    }

    fn retriever_options(&self) -> RetrieverOptions {
        RetrieverOptions {
            top_k: self.config.rag.top_k,
            relative_floor: self.config.rag.similarity_threshold,
        }
    }

    // ------------------------------------------------------------------ datos

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
        self.active_session = None;
        self.clear_chunks_panel();
        self.results.clear();
        self.search_note = None;
        self.reload_documents();
        self.reload_sessions();
    }

    /// Cierra el panel lateral de fragmentos y olvida lo seleccionado.
    fn clear_chunks_panel(&mut self) {
        self.chunks_preview.clear();
        self.selected_document = None;
        self.selected_chunk_id = None;
        self.chunks_panel_open = false;
        self.chunks_panel_title.clear();
    }

    fn reload_documents(&mut self) {
        self.documents.clear();
        self.clear_chunks_panel();

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
                self.selected_chunk_id = None;
                self.chunks_panel_title = relative_path.to_string();
                // Explorar un documento del índice muestra todos sus fragmentos.
                self.chunks_panel_open = true;
            }
            Err(error) => self.status = format!("No se pudieron cargar los fragmentos: {error}"),
        }
    }

    /// Busca el fragmento al que apunta una cita.
    ///
    /// La cita puede venir de la carpeta indexada o de un archivo adjunto al
    /// chat. Si el índice se reindexó con otro troceado (por ejemplo, antes de
    /// indexar las tablas fila a fila), la cita guardada ya no coincide con
    /// ningún fragmento: se busca entonces el fragmento actual de la misma zona.
    fn cited_chunk(&self, citation: &Citation) -> Option<Chunk> {
        let folder_chunks = self.chunks_of_source(citation, false);
        if let Some(chunk) = folder_chunks
            .iter()
            .find(|chunk| citation.matches_chunk(chunk))
        {
            return Some(chunk.clone());
        }
        if let Some(chunk) = folder_chunks
            .iter()
            .find(|chunk| mismo_lugar(citation, chunk))
        {
            return Some(chunk.clone());
        }

        let session_chunks = self.chunks_of_source(citation, true);
        if let Some(chunk) = session_chunks
            .iter()
            .find(|chunk| citation.matches_chunk(chunk))
        {
            return Some(chunk.clone());
        }
        session_chunks
            .into_iter()
            .find(|chunk| mismo_lugar(citation, chunk))
    }

    /// Fragmentos de un origen (carpeta o adjuntos) para la ruta de la cita.
    fn chunks_of_source(&self, citation: &Citation, attachment: bool) -> Vec<Chunk> {
        let Some(storage) = self.storage.as_ref() else {
            return Vec::new();
        };
        if attachment {
            let Some(session_id) = self.active_session.as_ref().map(|session| &session.id) else {
                return Vec::new();
            };
            storage
                .list_session_chunks_for_path(session_id, &citation.source_path)
                .unwrap_or_default()
        } else {
            let Some(folder_id) = self.selected_folder.as_deref() else {
                return Vec::new();
            };
            storage
                .list_chunks_for_path(folder_id, &citation.source_path)
                .unwrap_or_default()
        }
    }

    /// Abre el panel lateral con el fragmento al que apunta la cita.
    fn open_citation(&mut self, citation: &Citation) {
        match self.cited_chunk(citation) {
            Some(chunk) => {
                let exact = citation.matches_chunk(&chunk);
                // El título es el del fragmento que se muestra de verdad, para que
                // no anuncie una ubicación que ya no corresponde.
                self.chunks_panel_title = Citation::from_chunk(&chunk, 0.0).label();
                self.selected_document = Some(citation.source_path.clone());
                self.selected_chunk_id = Some(chunk.id.clone());
                self.chunks_preview = vec![chunk];
                self.chunks_panel_open = true;
                if !exact {
                    self.status = format!(
                        "La cita se guardó con otro índice; se muestra el fragmento actual de {}.",
                        citation.label()
                    );
                }
            }
            None => {
                self.status = format!(
                    "No se encontró el fragmento citado en {}.",
                    citation.label()
                );
            }
        }
    }

    // --------------------------------------------------------------- sesiones

    fn reload_sessions(&mut self) {
        self.sessions.clear();
        let Some(folder_id) = self.selected_folder.clone() else {
            return;
        };
        let Some(storage) = &self.storage else {
            return;
        };
        match storage.list_sessions(&folder_id) {
            Ok(sessions) => self.sessions = sessions,
            Err(error) => self.status = format!("No se pudieron listar las sesiones: {error}"),
        }
    }

    fn new_session(&mut self) {
        let Some(folder_id) = self.selected_folder.clone() else {
            self.status = "Selecciona una carpeta antes de crear una sesión.".to_string();
            return;
        };

        let provider = ProviderId::from_key(&self.config.model.default_provider)
            .unwrap_or(ProviderId::DeepSeek);
        let model = if self.config.model.default_model.trim().is_empty() {
            provider.default_model().to_string()
        } else {
            self.config.model.default_model.clone()
        };

        let session = Session::new(folder_id, provider.key(), model);
        let Some(storage) = &self.storage else {
            return;
        };
        if let Err(error) = storage.upsert_session(&session) {
            self.status = format!("No se pudo crear la sesión: {error}");
            return;
        }

        self.status = format!("Sesión «{}» creada.", session.title);
        self.select_session(session.id);
        self.reload_sessions();
    }

    fn select_session(&mut self, id: String) {
        let Some(storage) = &self.storage else {
            return;
        };
        match storage.get_session(&id) {
            Ok(Some(session)) => {
                let provider =
                    ProviderId::from_key(&session.provider).unwrap_or(ProviderId::DeepSeek);
                self.selected_folder = Some(session.folder_id.clone());
                self.messages = storage.list_messages(&id).unwrap_or_default();
                self.artifacts = storage.list_artifacts_by_session(&id).unwrap_or_default();
                self.attachments = storage.list_session_documents(&id).unwrap_or_default();
                self.refresh_rendered();
                self.active_session = Some(session);
                self.chat_input.clear();
                self.chat_error = None;
                self.streaming = None;
                self.results.clear();
                self.search_note = None;
                self.clear_chunks_panel();
                // Si al proveedor de la sesión le falta la clave, se avisa con la
                // alerta flotante (HU-23).
                if !self.provider_ready(provider) {
                    self.ask_api_key(provider);
                }
            }
            Ok(None) => self.status = "La sesión ya no existe.".to_string(),
            Err(error) => self.status = format!("No se pudo abrir la sesión: {error}"),
        }
    }

    fn rename_session(&mut self, id: &str, title: &str) {
        let Some(storage) = &self.storage else {
            return;
        };
        if let Err(error) = storage.rename_session(id, title) {
            self.status = format!("No se pudo renombrar la sesión: {error}");
            return;
        }
        if let Some(active) = self.active_session.as_mut() {
            if active.id == id {
                active.title = title.to_string();
            }
        }
        self.reload_sessions();
        self.status = "Sesión renombrada.".to_string();
    }

    fn delete_session(&mut self, id: &str) {
        let Some(storage) = &self.storage else {
            return;
        };
        if let Err(error) = storage.delete_session(id) {
            self.status = format!("No se pudo eliminar la sesión: {error}");
            return;
        }
        // Los índices de los adjuntos se borran en cascada con la sesión; los
        // archivos almacenados se retiran aquí.
        let _ = std::fs::remove_dir_all(self.paths.attachments_dir().join(id));
        self.search_cache.invalidate_session(id);
        if self.active_session.as_ref().is_some_and(|s| s.id == id) {
            self.active_session = None;
            self.messages.clear();
            self.rendered.clear();
            self.attachments.clear();
            self.streaming = None;
        }
        self.reload_sessions();
        self.status = "Sesión eliminada.".to_string();
    }

    /// Relee los adjuntos de la sesión activa.
    fn reload_attachments(&mut self, session_id: &str) {
        self.attachments.clear();
        let Some(storage) = &self.storage else {
            return;
        };
        match storage.list_session_documents(session_id) {
            Ok(documents) => self.attachments = documents,
            Err(error) => self.status = format!("No se pudieron listar los adjuntos: {error}"),
        }
    }

    /// Pide archivos y los adjunta al chat activo (HU-34).
    fn attach_files(&mut self) {
        let Some(session) = self.active_session.clone() else {
            self.status = "Abre un chat antes de adjuntar archivos.".to_string();
            return;
        };

        let extensions = raggy_core::indexable_extensions();
        let Some(files) = rfd::FileDialog::new()
            .set_title("Adjunta archivos al chat")
            .add_filter("Documentos y código", &extensions)
            .pick_files()
        else {
            return;
        };

        let directory = self.paths.attachments_dir().join(&session.id);
        let options = self.index_options();
        let summary = {
            let Some(storage) = self.storage.as_mut() else {
                return;
            };
            match raggy_core::attach_files(storage, &session.id, &directory, &files, &options) {
                Ok(summary) => summary,
                Err(error) => {
                    self.status = format!("No se pudieron adjuntar los archivos: {error}");
                    return;
                }
            }
        };

        if summary.is_empty() {
            self.status = match summary.skipped.first() {
                Some(skip) => format!(
                    "No se adjuntó nada: {} {}",
                    skip.path.display(),
                    skip.reason
                ),
                None => "No se adjuntó ningún archivo.".to_string(),
            };
        } else {
            self.status = format!("{} adjunto(s) al chat; indexando…", summary.attached.len());
            if !summary.replaced.is_empty() {
                self.status.push_str(&format!(
                    " {} reemplazó/reemplazaron un adjunto con el mismo contenido.",
                    summary.replaced.len()
                ));
            }
            if !summary.skipped.is_empty() {
                self.status
                    .push_str(&format!(" {} omitido(s).", summary.skipped.len()));
            }
        }

        self.search_cache.invalidate_session(&session.id);
        self.reload_attachments(&session.id);
    }

    /// Quita un adjunto del chat activo (HU-34).
    fn remove_attachment(&mut self, relative_path: &str) {
        let Some(session) = self.active_session.clone() else {
            return;
        };
        let directory = self.paths.attachments_dir().join(&session.id);
        let result = {
            let Some(storage) = self.storage.as_mut() else {
                return;
            };
            raggy_core::remove_attachment(storage, &session.id, &directory, relative_path)
                .map_err(|error| error.to_string())
        };
        if let Err(error) = result {
            self.status = format!("No se pudo quitar el adjunto: {error}");
            return;
        }
        self.search_cache.invalidate_session(&session.id);
        self.reload_attachments(&session.id);
        self.status = format!("Adjunto «{relative_path}» quitado.");
    }

    fn update_session_model(&mut self, provider: ProviderId, model: String) {
        let Some(session) = self.active_session.clone() else {
            return;
        };
        let Some(storage) = &self.storage else {
            return;
        };
        if let Err(error) = storage.update_session_model(&session.id, provider.key(), &model) {
            self.status = format!("No se pudo cambiar el modelo: {error}");
            return;
        }
        if let Some(active) = self.active_session.as_mut() {
            active.provider = provider.key().to_string();
            active.model = model;
        }
        self.reload_sessions();
    }

    fn set_session_fallback(&mut self, provider: Option<ProviderId>) {
        let Some(session) = self.active_session.clone() else {
            return;
        };
        let Some(storage) = &self.storage else {
            return;
        };
        let key = provider.map(ProviderId::key);
        if let Err(error) = storage.set_session_fallback(&session.id, key) {
            self.status = format!("No se pudo guardar el respaldo: {error}");
            return;
        }
        if let Some(active) = self.active_session.as_mut() {
            active.fallback_provider = key.map(str::to_string);
        }
        self.reload_sessions();
    }

    fn reload_messages(&mut self, session_id: &str) {
        if self
            .active_session
            .as_ref()
            .map(|session| session.id.as_str())
            != Some(session_id)
        {
            return;
        }
        if let Some(storage) = &self.storage {
            self.messages = storage.list_messages(session_id).unwrap_or_default();
        }
        self.refresh_rendered();
    }

    /// Recalcula los datos de dibujo de los mensajes cargados. Se hace al cargar
    /// una sesión o al terminar una respuesta, no en cada fotograma.
    fn refresh_rendered(&mut self) {
        self.rendered.clear();
        for message in &self.messages {
            self.rendered
                .insert(message.id.clone(), render_message(message));
        }
    }

    // ------------------------------------------------------------------ chat

    fn send_message(&mut self) {
        if self.streaming.is_some() {
            return;
        }
        let content = self.chat_input.trim().to_string();
        if content.is_empty() {
            return;
        }

        // Sin clave no se lanza la petición: se avisa con la alerta flotante.
        if let Some(session) = self.active_session.clone() {
            let provider = ProviderId::from_key(&session.provider).unwrap_or(ProviderId::DeepSeek);
            if resolve_key(self.secrets.as_ref(), provider).is_none() {
                self.ask_api_key(provider);
                return;
            }
        }

        let history = self.messages.clone();
        self.chat_input.clear();
        self.start_generation(content, history, true);
    }

    /// Reintenta el último mensaje del usuario con el proveedor de respaldo
    /// (HU-25). No persiste un mensaje nuevo.
    fn retry_with_fallback(&mut self) {
        let Some(session) = self.active_session.clone() else {
            return;
        };
        let Some(fallback) = session
            .fallback_provider
            .as_deref()
            .and_then(ProviderId::from_key)
        else {
            return;
        };

        self.update_session_model(fallback, fallback.default_model().to_string());

        let mut history = self.messages.clone();
        let Some(prompt) = history
            .iter()
            .rposition(|message| message.role == "user")
            .map(|position| {
                let content = history[position].content.clone();
                history.truncate(position);
                content
            })
        else {
            return;
        };

        self.status = format!("Reintentando con {}.", fallback.display_name());
        self.start_generation(prompt, history, false);
    }

    fn start_generation(&mut self, prompt: String, history: Vec<Message>, persist_user: bool) {
        let Some(session) = self.active_session.clone() else {
            return;
        };
        let provider = ProviderId::from_key(&session.provider).unwrap_or(ProviderId::DeepSeek);

        // El contexto se recupera de la carpeta de la sesión y de sus adjuntos
        // (HU-06, HU-34); nunca de otras sesiones.
        let options = self.retriever_options();
        let dimensions = self.config.rag.embedding_dimensions;
        let (blocks, citations) = match &self.storage {
            Some(storage) => {
                match self.search_cache.search_chat(
                    storage,
                    &session.folder_id,
                    &session.id,
                    &prompt,
                    &options,
                    dimensions,
                ) {
                    Ok(chunks) => context_from_retrieved(&chunks),
                    Err(error) => {
                        self.chat_error = Some(format!("Error al recuperar contexto: {error}"));
                        (Vec::new(), Vec::new())
                    }
                }
            }
            None => (Vec::new(), Vec::new()),
        };

        let request = compose_request(
            &session.model,
            self.config.model.temperature,
            self.config.model.max_tokens,
            &blocks,
            &artifacts_for_prompt(&self.artifacts, &prompt),
            &history,
            &prompt,
        );

        if persist_user {
            if let Some(storage) = &self.storage {
                let user_message = Message::new(&session.id, "user", &prompt, false, Vec::new());
                if let Err(error) = storage.add_message(&user_message) {
                    self.chat_error = Some(format!("No se pudo guardar el mensaje: {error}"));
                    return;
                }
                let _ = storage.touch_session(&session.id);
                self.messages = storage.list_messages(&session.id).unwrap_or_default();
                self.refresh_rendered();
            }
        }

        self.chat_error = None;
        self.last_failure = None;
        self.streaming = Some(StreamingState {
            session_id: session.id.clone(),
            text: String::new(),
        });

        match &self.chat_worker {
            Some(worker) => worker.generate(session.id, provider, request, citations),
            None => {
                self.streaming = None;
                self.chat_error = Some("El motor de chat no está disponible.".to_string());
            }
        }
    }

    fn drain_chat_events(&mut self) {
        let Some(worker) = &self.chat_worker else {
            return;
        };
        for event in worker.drain() {
            match event {
                ChatEvent::Delta { session_id, text } => {
                    if let Some(stream) = &mut self.streaming {
                        if stream.session_id == session_id {
                            stream.text.push_str(&text);
                        }
                    }
                }
                ChatEvent::Finished { session_id, .. } => {
                    self.streaming = None;
                    self.last_failure = None;
                    self.reload_messages(&session_id);
                }
                ChatEvent::Failed {
                    session_id,
                    message,
                    availability,
                } => {
                    self.streaming = None;
                    self.chat_error = Some(message);
                    self.last_failure = Some(Failure {
                        session_id: session_id.clone(),
                        availability,
                    });
                    self.reload_messages(&session_id);
                }
            }
        }
    }

    // ------------------------------------------------------------ artefactos

    fn refresh_artifacts(&mut self) {
        self.artifacts.clear();
        let Some(storage) = &self.storage else {
            return;
        };
        if let Some(session) = &self.active_session {
            self.artifacts = storage
                .list_artifacts_by_session(&session.id)
                .unwrap_or_default();
        }
    }

    fn artifact_scope_list(&self) -> Vec<Artifact> {
        let Some(storage) = &self.storage else {
            return Vec::new();
        };
        match self.artifact_scope {
            ArtifactScope::Session => self.artifacts.clone(),
            ArtifactScope::Folder => self
                .selected_folder
                .as_ref()
                .and_then(|folder| storage.list_artifacts_by_folder(folder).ok())
                .unwrap_or_default(),
        }
    }

    /// Extrae los bloques de código de un mensaje.
    fn code_blocks(&self, message_id: &str) -> Vec<CodeBlock> {
        self.messages
            .iter()
            .find(|message| message.id == message_id)
            .map(|message| extract_code_blocks(&message.content))
            .unwrap_or_default()
    }

    /// Nombre propuesto para un bloque: el editado, el declarado o uno genérico.
    fn proposed_name(&self, message_id: &str, index: usize, block: &CodeBlock) -> String {
        let key = format!("{message_id}:{index}");
        if let Some(edited) = self.code_edits.get(&key) {
            if !edited.trim().is_empty() {
                return edited.clone();
            }
        }
        block
            .filename
            .clone()
            .unwrap_or_else(|| suggest_filename(block.language.as_deref(), index))
    }

    fn pick_download_dir(&self) -> Option<PathBuf> {
        let mut dialog = rfd::FileDialog::new().set_title("Elige la carpeta de destino");
        if let Some(directory) = &self.last_download_dir {
            dialog = dialog.set_directory(directory);
        }
        dialog.pick_folder()
    }

    /// Registra un artefacto tras guardarlo en disco (HU-20).
    fn register_artifact(&mut self, root: &Path, path: &Path, block: &CodeBlock) {
        let Some(session) = self.active_session.clone() else {
            return;
        };
        let relative = relative_to(root, path);
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| relative.clone());
        let artifact = Artifact::new(
            &session.id,
            name,
            relative,
            path.to_string_lossy().to_string(),
            block.language.clone(),
            &block.content,
            Some(session.model.clone()),
        );

        if let Some(storage) = &self.storage {
            if let Err(error) = storage.add_artifact(&artifact) {
                self.status = format!("No se pudo registrar el artefacto: {error}");
                return;
            }
        }

        self.status = format!("Guardado en {}.", path.display());
        self.artifacts.push(artifact);
    }

    fn download_block(&mut self, message_id: String, block_index: usize) {
        let blocks = self.code_blocks(&message_id);
        let Some(block) = blocks.get(block_index).cloned() else {
            return;
        };
        let name = self.proposed_name(&message_id, block_index, &block);
        let Some(root) = self.pick_download_dir() else {
            return;
        };
        self.last_download_dir = Some(root.clone());

        match save_artifact(&root, &name, &block.content, ConflictPolicy::Fail) {
            Ok(SaveOutcome::Written(path)) | Ok(SaveOutcome::Renamed(path)) => {
                self.register_artifact(&root, &path, &block);
            }
            Ok(SaveOutcome::Exists(path)) => {
                self.pending_download = Some(PendingDownload {
                    name,
                    language: block.language.clone(),
                    content: block.content.clone(),
                    root,
                });
                self.status = format!("{} ya existe. Elige qué hacer.", path.display());
            }
            Err(error) => self.status = format!("No se pudo guardar: {error}"),
        }
    }

    fn resolve_conflict(&mut self, policy: ConflictPolicy) {
        let Some(pending) = self.pending_download.take() else {
            return;
        };
        let block = CodeBlock {
            language: pending.language.clone(),
            filename: Some(pending.name.clone()),
            content: pending.content.clone(),
            start_line: 0,
        };

        match save_artifact(&pending.root, &pending.name, &pending.content, policy) {
            Ok(SaveOutcome::Written(path)) | Ok(SaveOutcome::Renamed(path)) => {
                self.register_artifact(&pending.root, &path, &block);
            }
            Ok(SaveOutcome::Exists(path)) => {
                self.status = format!("{} sigue existiendo.", path.display());
            }
            Err(error) => self.status = format!("No se pudo guardar: {error}"),
        }
    }

    fn start_download_all(&mut self, message_id: String) {
        let blocks = self.code_blocks(&message_id);
        if blocks.is_empty() {
            return;
        }
        let Some(root) = self.pick_download_dir() else {
            return;
        };
        self.last_download_dir = Some(root.clone());

        let items = blocks
            .iter()
            .enumerate()
            .map(|(index, block)| BatchItem {
                relative: self.proposed_name(&message_id, index, block),
                language: block.language.clone(),
                content: block.content.clone(),
            })
            .collect();

        self.batch = Some(BatchDownload {
            session_id: self
                .active_session
                .as_ref()
                .map(|session| session.id.clone())
                .unwrap_or_default(),
            root,
            items,
            overwrite: false,
        });
    }

    fn confirm_batch(&mut self) {
        let Some(batch) = self.batch.take() else {
            return;
        };
        let policy = if batch.overwrite {
            ConflictPolicy::Overwrite
        } else {
            ConflictPolicy::Fail
        };

        let mut saved = 0usize;
        let mut skipped = 0usize;
        let mut failed = 0usize;
        let mut records: Vec<Artifact> = Vec::new();
        let model = self
            .active_session
            .as_ref()
            .map(|session| session.model.clone());

        for item in &batch.items {
            match save_artifact(&batch.root, &item.relative, &item.content, policy) {
                Ok(SaveOutcome::Written(path)) | Ok(SaveOutcome::Renamed(path)) => {
                    saved += 1;
                    let relative = relative_to(&batch.root, &path);
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| relative.clone());
                    records.push(Artifact::new(
                        &batch.session_id,
                        name,
                        relative,
                        path.to_string_lossy().to_string(),
                        item.language.clone(),
                        &item.content,
                        model.clone(),
                    ));
                }
                Ok(SaveOutcome::Exists(_)) => skipped += 1,
                Err(error) => {
                    failed += 1;
                    tracing::warn!("no se pudo guardar {}: {error}", item.relative);
                }
            }
        }

        if let Some(storage) = &self.storage {
            for artifact in &records {
                if let Err(error) = storage.add_artifact(artifact) {
                    tracing::warn!("no se pudo registrar el artefacto: {error}");
                }
            }
        }
        self.artifacts.extend(records);

        self.status = format!(
            "Descarga por lote: {saved} guardados, {skipped} ya existían, {failed} con error."
        );
    }

    /// Abre la carpeta que contiene un archivo con el explorador del sistema.
    fn open_location(path: &Path) {
        let directory = path.parent().unwrap_or(path);
        let command = if cfg!(target_os = "macos") {
            "open"
        } else if cfg!(target_os = "windows") {
            "explorer"
        } else {
            "xdg-open"
        };
        if let Err(error) = std::process::Command::new(command).arg(directory).spawn() {
            tracing::warn!("no se pudo abrir {}: {error}", directory.display());
        }
    }

    fn toggle_artifact_archived(&mut self, id: &str, archived: bool) {
        if let Some(storage) = &self.storage {
            let _ = storage.set_artifact_archived(id, archived);
        }
        if let Some(artifact) = self.artifacts.iter_mut().find(|artifact| artifact.id == id) {
            artifact.archived = archived;
        }
    }

    fn delete_artifact(&mut self, id: &str) {
        if let Some(storage) = &self.storage {
            let _ = storage.delete_artifact(id);
        }
        self.artifacts.retain(|artifact| artifact.id != id);
        self.status = "Registro eliminado (el archivo en disco no se toca).".to_string();
    }

    /// Exporta el árbol de artefactos como lista de texto (HU-22).
    fn export_tree(&mut self) {
        let artifacts = self.artifact_scope_list();
        if artifacts.is_empty() {
            self.status = "No hay artefactos que exportar.".to_string();
            return;
        }

        let mut dialog = rfd::FileDialog::new()
            .set_title("Exportar lista de artefactos")
            .set_file_name("artefactos.txt");
        if let Some(directory) = &self.last_download_dir {
            dialog = dialog.set_directory(directory);
        }
        let Some(path) = dialog.save_file() else {
            return;
        };

        let mut content = String::from("# Artefactos generados por Raggy\n");
        for artifact in &artifacts {
            content.push_str(&format!(
                "{}  ({} · {})\n",
                artifact.relative_path,
                artifact.language.clone().unwrap_or_else(|| "?".to_string()),
                short_timestamp(&artifact.created_at)
            ));
        }

        match std::fs::write(&path, content) {
            Ok(()) => self.status = format!("Lista exportada a {}.", path.display()),
            Err(error) => self.status = format!("No se pudo exportar: {error}"),
        }
    }

    // ----------------------------------------------------------- indexación

    fn add_folder(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Selecciona una carpeta con Markdown o código")
            .pick_folder()
        else {
            return;
        };
        self.register_folder(path);
    }

    /// Sube archivos de código a la carpeta gestionada y la reindexa.
    fn upload_files(&mut self) {
        let extensions = raggy_core::indexable_extensions();
        let Some(files) = rfd::FileDialog::new()
            .set_title("Sube archivos de código para analizar")
            .add_filter("Documentos y código", &extensions)
            .pick_files()
        else {
            return;
        };

        let uploads = self.paths.uploads_dir();
        let summary = match raggy_core::import_files(&uploads, &files) {
            Ok(summary) => summary,
            Err(error) => {
                self.status = format!("No se pudieron subir los archivos: {error}");
                return;
            }
        };

        if summary.is_empty() {
            self.status = "No se subió ningún archivo.".to_string();
            return;
        }

        let mut status = format!(
            "{} archivo(s) añadidos a «Archivos subidos»; indexando…",
            summary.imported.len()
        );
        if !summary.replaced.is_empty() {
            status = format!(
                "{status} {} reemplazó/reemplazaron un archivo anterior con el mismo nombre.",
                summary.replaced.len()
            );
        }
        if !summary.skipped.is_empty() {
            status = format!("{status} {} omitido(s).", summary.skipped.len());
        }
        self.status = status;
        self.ensure_uploads_folder();
    }

    /// Busca o crea la carpeta de subidas y la indexa.
    fn ensure_uploads_folder(&mut self) {
        let uploads = self.paths.uploads_dir();
        let path_str = uploads.to_string_lossy().to_string();

        let existing = match &self.storage {
            Some(storage) => storage.find_folder_by_path(&path_str).ok().flatten(),
            None => None,
        };

        let folder = match existing {
            Some(folder) => folder,
            None => {
                let folder = Folder::new(&path_str, "Archivos subidos");
                let stored = self
                    .storage
                    .as_ref()
                    .is_some_and(|storage| storage.upsert_folder(&folder).is_ok());
                if !stored {
                    self.status = "La base de datos no está disponible.".to_string();
                    return;
                }
                folder
            }
        };

        if !self
            .entries
            .iter()
            .any(|entry| entry.folder.id == folder.id)
        {
            self.entries.push(FolderEntry {
                folder: folder.clone(),
                chunks: 0,
                progress: Some((0, 0)),
                message: None,
                error: None,
            });
            self.sync_watcher();
        }

        self.select_folder(folder.id.clone());
        self.start_index(&folder.id, uploads);
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
        let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.folder.id == folder_id)
        else {
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
        self.search_cache.invalidate_folder(id);
        if self.selected_folder.as_deref() == Some(id) {
            self.selected_folder = None;
            self.active_session = None;
            self.sessions.clear();
            self.messages.clear();
            self.rendered.clear();
            self.reload_documents();
        }
        self.last_watch.remove(id);
        self.sync_watcher();
        self.status = "Carpeta eliminada del índice.".to_string();
    }

    // ---------------------------------------------------------------- eventos

    fn drain_events(&mut self) {
        let Some(worker) = &self.worker else {
            return;
        };
        for event in worker.drain() {
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
                    self.search_cache.invalidate_folder(&summary.folder_id);
                    if let Some(entry) = self
                        .entries
                        .iter_mut()
                        .find(|entry| entry.folder.id == summary.folder_id)
                    {
                        entry.progress = None;
                        entry.error = None;
                        entry.message = Some(format!(
                            "{} indexados · {} sin cambios · {} retirados · {} fallidos · {} fragmentos",
                            summary.files_indexed,
                            summary.files_skipped,
                            summary.files_removed,
                            summary.files_failed,
                            summary.chunks_written
                        ));
                    }
                    self.index_warnings = summary.warnings.clone();
                    self.status = if summary.warnings.is_empty() {
                        format!(
                            "Indexación completada: {} archivos actualizados.",
                            summary.files_indexed
                        )
                    } else {
                        format!(
                            "Indexación completada con {} avisos (ver Configuración › OCR y conversión).",
                            summary.warnings.len()
                        )
                    };
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
                IndexEvent::AttachmentsReindexed { count } => {
                    // Los adjuntos se reindexaron con otro pipeline: la caché de
                    // búsqueda por sesión queda obsoleta.
                    self.search_cache = SearchCache::default();
                    self.status = format!(
                        "{count} adjunto(s) de los chats reindexados con el pipeline nuevo."
                    );
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
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                let dark = ui.visuals().dark_mode;
                let accent = crate::theme::accent(dark);
                let tint = ui.visuals().weak_text_color();

                ui.add_space(4.0);
                crate::icons::icon(ui, crate::icons::folder(), accent, 17.0);
                ui.add_space(2.0);
                ui.label(egui::RichText::new(APP_NAME).strong())
                    .on_hover_text(format!("Datos: {}", data_dir.display()));
                ui.weak(egui::RichText::new(format!("v{APP_VERSION}")).small());
                ui.add_space(4.0);
                ui.separator();

                if crate::icons::labelled_button(
                    ui,
                    crate::icons::folder_plus(),
                    "Añadir carpeta",
                    tint,
                )
                .clicked()
                {
                    actions.add = true;
                }
                if crate::icons::labelled_button(ui, crate::icons::upload(), "Subir archivos", tint)
                    .clicked()
                {
                    actions.upload = true;
                }
                if crate::icons::labelled_button(
                    ui,
                    crate::icons::refresh(),
                    "Reindexar todo",
                    tint,
                )
                .clicked()
                {
                    actions.reindex_all = true;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(6.0);
                    if crate::icons::icon_button(
                        ui,
                        crate::icons::settings(),
                        "Configuración",
                        tint,
                    )
                    .clicked()
                    {
                        actions.settings = true;
                    }
                    if crate::icons::icon_button(ui, crate::icons::key(), "Claves de API", tint)
                        .clicked()
                    {
                        actions.keys = true;
                    }
                    if crate::icons::icon_button(ui, crate::icons::archive(), "Artefactos", tint)
                        .clicked()
                    {
                        actions.artifacts = true;
                    }
                });
            });
            ui.add_space(5.0);
        });
        actions
    }

    fn sidebar(
        ui: &mut egui::Ui,
        entries: &[FolderEntry],
        selected_folder: Option<&str>,
        sessions: &[Session],
        active_session: Option<&str>,
    ) -> SidebarActions {
        let mut actions = SidebarActions::default();

        egui::Panel::left("sidebar")
            .default_size(320.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("sidebar")
                    .show(ui, |ui| {
                        ui.add_space(10.0);

                        section_title(ui, "CARPETAS");
                        ui.add_space(4.0);

                        if entries.is_empty() {
                            ui.add_space(2.0);
                            hint(ui, "Aún no hay carpetas. Usa «Añadir carpeta».");
                            ui.add_space(6.0);
                        }

                        for entry in entries {
                            let is_selected = selected_folder == Some(entry.folder.id.as_str());
                            let dark = ui.visuals().dark_mode;

                            card(ui, is_selected, |ui| {
                                ui.horizontal(|ui| {
                                    let tint = tint_for(ui, is_selected);
                                    crate::icons::icon(ui, crate::icons::folder(), tint, 15.0);
                                    ui.add_space(2.0);

                                    let title = ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&entry.folder.name).strong(),
                                        )
                                        .sense(egui::Sense::click()),
                                    );
                                    if title.clicked() {
                                        actions.select_folder = Some(entry.folder.id.clone());
                                    }

                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if crate::icons::icon_button(
                                                ui,
                                                crate::icons::trash(),
                                                "Eliminar del índice",
                                                tint,
                                            )
                                            .clicked()
                                            {
                                                actions.delete_folder =
                                                    Some(entry.folder.id.clone());
                                            }
                                            if crate::icons::icon_button(
                                                ui,
                                                crate::icons::refresh(),
                                                "Reindexar",
                                                tint,
                                            )
                                            .clicked()
                                            {
                                                actions.reindex = Some(entry.folder.id.clone());
                                            }
                                        },
                                    );
                                });

                                ui.horizontal(|ui| {
                                    let (color, text) = match (&entry.progress, &entry.error) {
                                        (Some((done, total)), _) => (
                                            crate::theme::accent(dark),
                                            format!("Indexando… {done}/{total}"),
                                        ),
                                        (None, Some(error)) => (
                                            crate::theme::error_color(dark),
                                            format!("Error: {error}"),
                                        ),
                                        (None, None) => (
                                            crate::theme::success_color(dark),
                                            format!(
                                                "{} archivos · {} fragmentos",
                                                entry.folder.file_count, entry.chunks
                                            ),
                                        ),
                                    };
                                    status_dot(ui, color);
                                    ui.weak(egui::RichText::new(text).small());
                                });

                                if let Some(message) = &entry.message {
                                    ui.horizontal(|ui| {
                                        ui.add_space(10.0);
                                        ui.weak(egui::RichText::new(message).small());
                                    });
                                }
                            });
                        }

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);

                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            ui.label(egui::RichText::new("SESIONES").small().strong());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.add_space(10.0);
                                    if crate::icons::icon_button(
                                        ui,
                                        crate::icons::plus(),
                                        "Nueva sesión",
                                        crate::theme::accent(ui.visuals().dark_mode),
                                    )
                                    .clicked()
                                    {
                                        actions.new_session = true;
                                    }
                                },
                            );
                        });
                        ui.add_space(4.0);

                        if sessions.is_empty() {
                            ui.add_space(2.0);
                            hint(ui, "Sin sesiones en esta carpeta.");
                            ui.add_space(6.0);
                        }

                        for session in sessions {
                            let is_active = active_session == Some(session.id.as_str());

                            card(ui, is_active, |ui| {
                                ui.horizontal(|ui| {
                                    let tint = tint_for(ui, is_active);
                                    crate::icons::icon(ui, crate::icons::chat(), tint, 15.0);
                                    ui.add_space(2.0);

                                    let title = ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&session.title).strong(),
                                        )
                                        .sense(egui::Sense::click()),
                                    );
                                    if title.clicked() {
                                        actions.select_session = Some(session.id.clone());
                                    }

                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if crate::icons::icon_button(
                                                ui,
                                                crate::icons::trash(),
                                                "Eliminar sesión",
                                                tint,
                                            )
                                            .clicked()
                                            {
                                                actions.delete_session = Some(session.id.clone());
                                            }
                                            if crate::icons::icon_button(
                                                ui,
                                                crate::icons::pencil(),
                                                "Renombrar sesión",
                                                tint,
                                            )
                                            .clicked()
                                            {
                                                actions.rename_session = Some(session.id.clone());
                                            }
                                        },
                                    );
                                });

                                ui.horizontal(|ui| {
                                    ui.add_space(10.0);
                                    ui.weak(
                                        egui::RichText::new(format!(
                                            "{} · {}",
                                            session.provider, session.model
                                        ))
                                        .small(),
                                    );
                                });
                            });
                        }

                        ui.add_space(12.0);
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
                ui.colored_label(crate::theme::error_color(ui.visuals().dark_mode), fatal);
                return;
            }

            if self.active_session.is_some() {
                actions.extend(self.chat_view(ui));
                return;
            }

            if self.selected_folder.is_none() {
                ui.vertical_centered(|ui| {
                    ui.add_space(96.0);
                    ui.heading(APP_NAME);
                    ui.add_space(8.0);
                    ui.label("Añade una carpeta con documentos compatibles para indexarla.");
                    ui.add_space(16.0);
                    ui.label("Después podrás buscar en su contenido o iniciar una sesión de chat.");
                });
                return;
            }

            actions.extend(self.explore_view(ui));
        });
        actions
    }

    /// Vista de exploración: búsqueda y archivos de la carpeta.
    fn explore_view(&mut self, ui: &mut egui::Ui) -> Vec<CentralAction> {
        let mut actions = Vec::new();

        if let Some(folder_id) = self.selected_folder.clone() {
            if let Some(entry) = self
                .entries
                .iter()
                .find(|entry| entry.folder.id == folder_id)
            {
                ui.add_space(6.0);
                ui.heading(&entry.folder.name);
                ui.small(&entry.folder.path);
            }
        }

        ui.add_space(6.0);
        let mut query = std::mem::take(&mut self.query);
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut query)
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
        self.query = query;

        // El panel de fragmentos/citas, debajo de la barra (carpeta y buscador).
        self.chunks_panel(ui);

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
                        let heading = metadata.heading.clone().unwrap_or_else(|| "—".to_string());
                        let citation = result.citation();
                        let label = format!(
                            "{}. [{:.2}] {}  ·  {}",
                            index + 1,
                            result.score,
                            citation.label(),
                            heading
                        );
                        if ui.selectable_label(false, label).clicked() {
                            actions.push(CentralAction::OpenCitation(citation));
                        }
                        ui.small(snippet(&result.chunk.content));
                        ui.add_space(6.0);
                    }
                });
            return actions;
        }

        if self.documents.is_empty() {
            ui.label("Sin archivos indexados en esta carpeta.");
            return actions;
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
                            let is_selected =
                                self.selected_document.as_deref() == Some(&document.relative_path);
                            if ui
                                .selectable_label(is_selected, &document.relative_path)
                                .clicked()
                            {
                                actions.push(CentralAction::OpenDocument(
                                    document.relative_path.clone(),
                                ));
                            }
                            if document.chunk_count == 0 {
                                ui.colored_label(
                                    crate::theme::warning_color(ui.visuals().dark_mode),
                                    "0",
                                )
                                .on_hover_text(
                                    "No se extrajo texto de este archivo (puede estar \
                                     escaneado, dañado o necesitar una herramienta). \
                                     Revisa los avisos en Configuración.",
                                );
                            } else {
                                ui.label(document.chunk_count.to_string());
                            }
                            ui.label(document.size_bytes.to_string());
                            ui.label(short_timestamp(&document.indexed_at));
                            ui.end_row();
                        }
                    });
            });

        actions
    }

    /// Vista de chat de la sesión activa, con el compositor fijo abajo.
    fn chat_view(&mut self, ui: &mut egui::Ui) -> Vec<CentralAction> {
        let mut actions = Vec::new();
        let Some(session) = self.active_session.clone() else {
            return actions;
        };

        let provider = ProviderId::from_key(&session.provider).unwrap_or(ProviderId::DeepSeek);
        let is_streaming = self.streaming.is_some();
        let streaming_text = self
            .streaming
            .as_ref()
            .filter(|stream| stream.session_id == session.id)
            .map(|stream| stream.text.clone());
        let dark = ui.visuals().dark_mode;

        // --- cabecera: sesión, proveedor, modelo, respaldo y acciones ---
        ui.add_space(6.0);
        ui.scope(|ui| {
            // Todos los controles de la fila comparten altura para quedar centrados
            // en la misma línea (HU de sesión; el título, los desplegables y los
            // botones dejan de bailar entre sí).
            ui.spacing_mut().interact_size.y = SESSION_HEADER_HEIGHT;
            ui.horizontal(|ui| {
                ui.heading(&session.title);
                ui.separator();

                let mut selected_provider = provider;
                egui::ComboBox::from_id_salt("provider_selector")
                    .selected_text(provider.display_name())
                    .show_ui(ui, |ui| {
                        for candidate in ProviderId::ALL {
                            ui.selectable_value(
                                &mut selected_provider,
                                candidate,
                                candidate.display_name(),
                            );
                        }
                    });

                let mut selected_model = session.model.clone();
                // El selector usa los modelos que detecta el proveedor con la
                // clave del usuario, no una lista fija (HU-24).
                let detected = self.detected_models.get(&selected_provider).cloned();
                let detecting = self.detecting_models.contains(&selected_provider);
                let model_error = self.model_errors.get(&selected_provider).cloned();
                let provider_missing = !self.provider_ready(selected_provider);
                egui::ComboBox::from_id_salt("model_selector")
                    .selected_text(session.model.clone())
                    .show_ui(ui, |ui| {
                        match detected {
                            Some(models) => {
                                // Si el modelo guardado no está en la lista
                                // detectada, se ofrece primero para no perderlo.
                                if !models.contains(&session.model) {
                                    ui.selectable_value(
                                        &mut selected_model,
                                        session.model.clone(),
                                        session.model.clone(),
                                    );
                                    ui.separator();
                                }
                                for candidate in models {
                                    ui.selectable_value(
                                        &mut selected_model,
                                        candidate.clone(),
                                        candidate,
                                    );
                                }
                            }
                            None => {
                                // Sin detección se ofrece solo el modelo actual y
                                // una pista de por qué no hay lista.
                                if let Some(error) = &model_error {
                                    ui.weak(format!("No se detectaron modelos: {error}"));
                                } else if detecting {
                                    ui.weak("Detectando modelos…");
                                } else if provider_missing {
                                    ui.weak("Sin API Key: configura la clave para ver los modelos");
                                } else {
                                    ui.weak("Detectando modelos…");
                                }
                                ui.selectable_value(
                                    &mut selected_model,
                                    session.model.clone(),
                                    session.model.clone(),
                                );
                            }
                        }
                    });

                if selected_provider != provider || selected_model != session.model {
                    actions.push(CentralAction::UpdateModel {
                        provider: selected_provider,
                        model: selected_model,
                    });
                }

                ui.separator();
                let mut selected_fallback = session
                    .fallback_provider
                    .as_deref()
                    .and_then(ProviderId::from_key);
                egui::ComboBox::from_id_salt("fallback_selector")
                    .selected_text(match selected_fallback {
                        Some(fallback) => format!("Respaldo: {}", fallback.display_name()),
                        None => "Sin respaldo".to_string(),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut selected_fallback, None, "Sin respaldo");
                        for candidate in ProviderId::ALL {
                            if candidate != provider {
                                ui.selectable_value(
                                    &mut selected_fallback,
                                    Some(candidate),
                                    candidate.display_name(),
                                );
                            }
                        }
                    });
                let previous_fallback = session
                    .fallback_provider
                    .as_deref()
                    .and_then(ProviderId::from_key);
                if selected_fallback != previous_fallback {
                    actions.push(CentralAction::UpdateFallback {
                        provider: selected_fallback,
                    });
                }

                if !self.provider_ready(provider) {
                    // El aviso en sí es una ventana flotante; aquí solo queda un
                    // botón discreto para volver a abrirla.
                    let alert_text = egui::RichText::new("Falta la API Key")
                        .color(crate::theme::warning_color(dark));
                    if ui
                        .button(alert_text)
                        .on_hover_text(format!(
                            "Configura {} para usar {}",
                            provider.api_key_env(),
                            provider.display_name()
                        ))
                        .clicked()
                    {
                        self.ask_api_key(provider);
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Exportar sesión").clicked() {
                        actions.push(CentralAction::ExportSession);
                    }
                    if ui.button("Artefactos").clicked() {
                        self.artifacts_open = true;
                    }
                    if crate::icons::labelled_button(
                        ui,
                        crate::icons::attach(),
                        "Adjuntar",
                        ui.visuals().weak_text_color(),
                    )
                    .on_hover_text(
                        "Adjunta archivos a este chat para buscarlos junto con la carpeta",
                    )
                    .clicked()
                    {
                        actions.push(CentralAction::AttachFiles);
                    }
                });
            });
        });
        ui.add_space(6.0);
        ui.separator();

        // --- adjuntos del chat (HU-34) ---
        if !self.attachments.is_empty() {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.small("Adjuntos:");
                for document in &self.attachments {
                    let label = format!(
                        "{} ({})",
                        document.relative_path,
                        human_size(document.size_bytes)
                    );
                    if ui
                        .button(format!("{label}  ✕"))
                        .on_hover_text("Quitar este adjunto del chat")
                        .clicked()
                    {
                        actions.push(CentralAction::RemoveAttachment(
                            document.relative_path.clone(),
                        ));
                    }
                }
            });
            ui.add_space(4.0);
        }

        // --- compositor fijo en la parte inferior ---
        let mut input = std::mem::take(&mut self.chat_input);
        let mut send = false;
        let mut stop = false;
        let mut focused = false;

        egui::Panel::bottom("chat_composer").show(ui, |ui| {
            // Relleno simétrico: la fila (caja + botón) queda centrada en altura
            // entre los delimitadores superior e inferior del área.
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(8, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let button_width = 118.0;
                        let gap = ui.spacing().item_spacing.x;
                        let text_width = (ui.available_width() - button_width - gap).max(160.0);

                        let response = ui.add(
                            egui::TextEdit::multiline(&mut input)
                                .desired_width(text_width)
                                .min_size(egui::vec2(text_width, COMPOSER_INPUT_HEIGHT))
                                .margin(egui::Margin::symmetric(8, 6))
                                .hint_text("Escribe tu pregunta…"),
                        );
                        focused = response.has_focus();

                        let label = if is_streaming { "Detener" } else { "Enviar" };
                        let button = if is_streaming {
                            egui::Button::new(egui::RichText::new(label).size(15.0))
                                .fill(crate::theme::secondary_button(dark))
                                .corner_radius(egui::CornerRadius::same(8))
                                .min_size(egui::vec2(button_width, COMPOSER_INPUT_HEIGHT))
                        } else {
                            egui::Button::new(
                                egui::RichText::new(label)
                                    .size(15.0)
                                    .color(crate::theme::on_accent(dark)),
                            )
                            .fill(crate::theme::accent(dark))
                            .corner_radius(egui::CornerRadius::same(8))
                            .min_size(egui::vec2(button_width, COMPOSER_INPUT_HEIGHT))
                        };
                        if ui.add(button).clicked() {
                            if is_streaming {
                                stop = true;
                            } else {
                                send = true;
                            }
                        }
                    });

                    if let Some(error) = &self.chat_error {
                        ui.add_space(6.0);
                        ui.colored_label(crate::theme::error_color(dark), error);
                        if self.retry_available(session.id.as_str()) {
                            if let Some(fallback) = session
                                .fallback_provider
                                .as_deref()
                                .and_then(ProviderId::from_key)
                            {
                                if ui
                                    .button(format!(
                                        "Usar {} y reintentar",
                                        fallback.display_name()
                                    ))
                                    .clicked()
                                {
                                    actions.push(CentralAction::RetryWithFallback);
                                }
                            }
                        }
                    }
                });
        });

        // El panel de fragmentos/citas va dentro del área del chat: al declararse
        // después de la barra de botones y del compositor, ambos conservan el ancho
        // completo y el panel queda por debajo de los botones.
        self.chunks_panel(ui);

        // Enter envía; Mayús+Intro inserta un salto de línea.
        let shift = ui.input(|input_state| input_state.modifiers.shift);
        if focused
            && !shift
            && !is_streaming
            && ui.input_mut(|input_state| {
                input_state.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
            })
        {
            send = true;
        }

        // --- conversación ---
        let mut pending_edits: Vec<(String, String)> = Vec::new();
        let mut pending_previews: Vec<String> = Vec::new();

        egui::ScrollArea::vertical()
            .id_salt("chat_messages")
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.messages.is_empty() && streaming_text.is_none() {
                    ui.add_space(24.0);
                    ui.weak("Escribe tu primera pregunta sobre esta carpeta.");
                    ui.add_space(12.0);
                }

                for message in &self.messages {
                    let is_user = message.role == "user";
                    // Si falta en la caché (caso raro), se calcula al vuelo.
                    let computed;
                    let rendered = if let Some(cached) = self.rendered.get(&message.id) {
                        cached
                    } else {
                        computed = render_message(message);
                        &computed
                    };

                    // La prosa va en la burbuja; el código, en su tarjeta.
                    let prose = rendered.prose.as_str();
                    if !prose.trim().is_empty() {
                        message_bubble(ui, prose.trim(), is_user);
                    }

                    if !is_user {
                        render_code_blocks(
                            ui,
                            &message.id,
                            &rendered.blocks,
                            &self.code_edits,
                            &self.previews,
                            &mut pending_edits,
                            &mut pending_previews,
                            &mut actions,
                        );

                        if rendered.citations.is_empty() {
                            ui.weak("Sin citas verificadas.");
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                ui.small("Fuentes verificadas:");
                                for citation in &rendered.citations {
                                    if ui.link(citation.label()).clicked() {
                                        actions.push(CentralAction::OpenCitation(citation.clone()));
                                    }
                                }
                            });
                        }
                    }

                    ui.add_space(10.0);
                }

                if let Some(text) = &streaming_text {
                    message_bubble(ui, text, false);
                    ui.weak("generando…");
                    ui.add_space(10.0);
                }
            });

        for (key, name) in pending_edits {
            self.code_edits.insert(key, name);
        }
        for key in pending_previews {
            let entry = self.previews.entry(key).or_insert(false);
            *entry = !*entry;
        }

        self.chat_input = input;

        if stop {
            actions.push(CentralAction::StopGeneration);
        } else if send {
            actions.push(CentralAction::SendMessage);
        }

        actions
    }

    /// ¿El último fallo permite ofrecer el proveedor de respaldo?
    fn retry_available(&self, session_id: &str) -> bool {
        self.last_failure
            .as_ref()
            .is_some_and(|failure| failure.session_id == session_id && failure.availability)
    }

    /// ¿El proveedor tiene credenciales listas para usar? (según la caché de claves)
    fn provider_ready(&self, provider: ProviderId) -> bool {
        matches!(
            self.key_status.get(&provider),
            Some(KeyStatus::Stored { .. } | KeyStatus::Environment { .. } | KeyStatus::Ok { .. })
        )
    }

    /// Panel lateral con los fragmentos de un archivo del índice o con el
    /// fragmento concreto al que apunta una cita. Se cierra con la ✕ o con Esc.
    fn chunks_panel(&mut self, ui: &mut egui::Ui) {
        if !self.chunks_panel_open {
            return;
        }

        let title = self.chunks_panel_title.clone();
        let single = self.chunks_preview.len() == 1;
        let mut selected_chunk_id = None;
        let mut close = false;
        egui::Panel::right("chunks_panel")
            .default_size(460.0)
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.strong(&title);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::icons::icon_button(
                            ui,
                            crate::icons::close(),
                            "Cerrar (Esc)",
                            ui.visuals().weak_text_color(),
                        )
                        .clicked()
                        {
                            close = true;
                        }
                    });
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.chunks_preview.is_empty() {
                        ui.label("Sin fragmentos.");
                        return;
                    }
                    for (index, chunk) in self.chunks_preview.iter().enumerate() {
                        let citation = Citation::from_chunk(chunk, 0.0);
                        // Al abrir una cita solo hay un fragmento: se etiqueta con
                        // la cita misma, sin numerarlo.
                        let label = if single {
                            citation.label()
                        } else {
                            format!("Fragmento {} · {}", index + 1, citation.label())
                        };
                        if ui
                            .selectable_label(
                                self.selected_chunk_id.as_deref() == Some(&chunk.id),
                                label,
                            )
                            .clicked()
                        {
                            selected_chunk_id = Some(chunk.id.clone());
                        }
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

        // Esc cierra el panel, como la ✕.
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            close = true;
        }

        if let Some(chunk_id) = selected_chunk_id {
            self.selected_chunk_id = Some(chunk_id);
        }
        if close {
            self.chunks_panel_open = false;
        }
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
                    "Se borrarán sus fragmentos, sus sesiones y sus mensajes. \
                     Los archivos en disco no se tocan.",
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

    fn session_dialog_window(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.session_dialog.take() else {
            return;
        };

        match dialog {
            SessionDialog::Rename { id, mut title } => {
                let mut open = true;
                let mut confirm = false;
                let mut cancel = false;

                egui::Window::new("Renombrar sesión")
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                    .show(ctx, |ui| {
                        ui.add(egui::TextEdit::singleline(&mut title).desired_width(280.0));
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Cancelar").clicked() {
                                cancel = true;
                            }
                            if ui.button("Guardar").clicked() {
                                confirm = true;
                            }
                        });
                    });

                if confirm {
                    let trimmed = title.trim();
                    if trimmed.is_empty() {
                        self.status = "El nombre de la sesión no puede estar vacío.".to_string();
                        self.session_dialog = Some(SessionDialog::Rename { id, title });
                    } else {
                        self.rename_session(&id, trimmed);
                    }
                } else if !cancel && open {
                    self.session_dialog = Some(SessionDialog::Rename { id, title });
                }
            }
            SessionDialog::Delete { id } => {
                let mut confirm = false;
                let mut cancel = false;

                egui::Window::new("Eliminar sesión")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                    .show(ctx, |ui| {
                        ui.label("¿Eliminar esta sesión y todo su historial?");
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
                    self.delete_session(&id);
                } else if cancel {
                    self.session_dialog = None;
                } else {
                    self.session_dialog = Some(SessionDialog::Delete { id });
                }
            }
        }
    }

    fn artifacts_window(&mut self, ctx: &egui::Context) {
        if !self.artifacts_open {
            return;
        }

        let mut open = true;
        let mut actions: Vec<ArtifactAction> = Vec::new();
        let mut scope_change: Option<ArtifactScope> = None;
        let mut export = false;
        let artifacts = self.artifact_scope_list();
        let scope = self.artifact_scope;

        egui::Window::new("Artefactos generados")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(760.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Ámbito:");
                    if ui
                        .selectable_label(scope == ArtifactScope::Session, "Sesión actual")
                        .clicked()
                    {
                        scope_change = Some(ArtifactScope::Session);
                    }
                    if ui
                        .selectable_label(scope == ArtifactScope::Folder, "Toda la carpeta")
                        .clicked()
                    {
                        scope_change = Some(ArtifactScope::Folder);
                    }
                    ui.separator();
                    if ui.button("Exportar lista").clicked() {
                        export = true;
                    }
                });
                ui.separator();

                if artifacts.is_empty() {
                    ui.label("Aún no has descargado código en este ámbito.");
                    return;
                }

                ui.small("Árbol de archivos generados (por ruta):");
                ui.add_space(4.0);

                egui::ScrollArea::vertical()
                    .id_salt("artifacts_tree")
                    .max_height(420.0)
                    .show(ui, |ui| {
                        let mut last_directory = String::new();

                        for artifact in &artifacts {
                            let directory = artifact
                                .relative_path
                                .rsplit_once('/')
                                .map(|(directory, _)| directory.to_string())
                                .unwrap_or_default();
                            if directory != last_directory {
                                if directory.is_empty() {
                                    ui.strong("(raíz)");
                                } else {
                                    ui.strong(format!("{directory}/"));
                                }
                                last_directory = directory;
                            }

                            let depth = artifact.relative_path.matches('/').count();
                            ui.horizontal(|ui| {
                                ui.add_space(depth as f32 * 16.0 + 16.0);
                                let name = artifact
                                    .relative_path
                                    .rsplit('/')
                                    .next()
                                    .unwrap_or(&artifact.relative_path);
                                ui.monospace(name);
                                if artifact.archived {
                                    ui.small("(archivado)");
                                }
                                ui.small(artifact.language.clone().unwrap_or_default());
                                ui.small(short_timestamp(&artifact.created_at));
                                if ui.small_button("Abrir ubicación").clicked() {
                                    actions
                                        .push(ArtifactAction::Open(PathBuf::from(&artifact.path)));
                                }
                                if ui
                                    .small_button(if artifact.archived {
                                        "Restaurar"
                                    } else {
                                        "Archivar"
                                    })
                                    .clicked()
                                {
                                    actions.push(ArtifactAction::Archive(
                                        artifact.id.clone(),
                                        !artifact.archived,
                                    ));
                                }
                                if ui.small_button("Eliminar registro").clicked() {
                                    actions.push(ArtifactAction::Delete(artifact.id.clone()));
                                }
                            });
                        }
                    });
            });

        if let Some(scope) = scope_change {
            self.artifact_scope = scope;
        }
        if export {
            self.export_tree();
        }
        for action in actions {
            match action {
                ArtifactAction::Open(path) => Self::open_location(&path),
                ArtifactAction::Archive(id, archived) => {
                    self.toggle_artifact_archived(&id, archived)
                }
                ArtifactAction::Delete(id) => self.delete_artifact(&id),
            }
        }
        if !open {
            self.artifacts_open = false;
        }
    }

    fn batch_dialog(&mut self, ctx: &egui::Context) {
        let Some(batch) = self.batch.as_mut() else {
            return;
        };

        let mut confirm = false;
        let mut cancel = false;
        let items: Vec<(String, Option<String>)> = batch
            .items
            .iter()
            .map(|item| (item.relative.clone(), item.language.clone()))
            .collect();
        let root = batch.root.clone();
        let mut overwrite = batch.overwrite;

        egui::Window::new("Descargar todo")
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!("Destino: {}", root.display()));
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .id_salt("batch_items")
                    .max_height(280.0)
                    .show(ui, |ui| {
                        for (relative, language) in &items {
                            ui.horizontal(|ui| {
                                ui.monospace(relative);
                                if let Some(language) = language {
                                    ui.small(language.clone());
                                }
                            });
                        }
                    });
                ui.add_space(6.0);
                ui.checkbox(&mut overwrite, "Sobrescribir los archivos que ya existan");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancelar").clicked() {
                        cancel = true;
                    }
                    if ui
                        .button(format!("Guardar {} archivos", items.len()))
                        .clicked()
                    {
                        confirm = true;
                    }
                });
            });

        if let Some(batch) = self.batch.as_mut() {
            batch.overwrite = overwrite;
        }

        if confirm {
            self.confirm_batch();
        } else if cancel {
            self.batch = None;
        }
    }

    fn conflict_dialog(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_download.as_ref() else {
            return;
        };

        let name = pending.name.clone();
        let root = pending.root.clone();
        let mut resolution: Option<ConflictPolicy> = None;

        egui::Window::new("El archivo ya existe")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!("«{name}» ya existe en {}", root.display()));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancelar").clicked() {
                        resolution = Some(ConflictPolicy::Fail);
                    }
                    if ui.button("Renombrar").clicked() {
                        resolution = Some(ConflictPolicy::Rename);
                    }
                    if ui.button("Sobrescribir").clicked() {
                        resolution = Some(ConflictPolicy::Overwrite);
                    }
                });
            });

        match resolution {
            Some(ConflictPolicy::Fail) => self.pending_download = None,
            Some(policy) => self.resolve_conflict(policy),
            None => {}
        }
    }

    fn keys_window(&mut self, ctx: &egui::Context) {
        if !self.keys_open {
            return;
        }

        let mut open = true;
        let mut actions: Vec<KeyAction> = Vec::new();
        let statuses = self.key_status.clone();
        let detected = self.detected_models.clone();
        let detecting = self.detecting_models.clone();
        let settings_path = self.paths.settings_path();
        let mut tab = self.key_tab;

        egui::Window::new("Claves de API")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .min_width(460.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.small(format!(
                    "Se guardan en {} (permisos restringidos) y nunca se muestran completas.",
                    settings_path.display()
                ));
                ui.add_space(8.0);

                // Una pestaña por proveedor; el punto marca los ya configurados.
                ui.horizontal_wrapped(|ui| {
                    for provider in ProviderId::ALL {
                        let configured = matches!(
                            statuses.get(&provider),
                            Some(KeyStatus::Stored { .. })
                                | Some(KeyStatus::Environment { .. })
                                | Some(KeyStatus::Ok { .. })
                        );
                        let label = if configured {
                            format!("{} ·", provider.display_name())
                        } else {
                            provider.display_name().to_string()
                        };
                        ui.selectable_value(&mut tab, provider, label);
                    }
                });
                ui.separator();
                ui.add_space(6.0);

                ui.heading(tab.display_name());
                ui.label(key_status_text(statuses.get(&tab)));
                ui.add_space(10.0);

                ui.label("Clave de API");
                let input = self.key_inputs.entry(tab).or_default();
                ui.add(
                    egui::TextEdit::singleline(input)
                        .password(true)
                        .desired_width(360.0)
                        .hint_text("pega la clave del proveedor"),
                );
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    if ui.button("Guardar").clicked() {
                        actions.push(KeyAction::Save(tab));
                    }
                    if ui.button("Probar conexión").clicked() {
                        actions.push(KeyAction::Test(tab));
                    }
                    if ui.button("Eliminar").clicked() {
                        actions.push(KeyAction::Delete(tab));
                    }
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(4.0);
                ui.small(format!(
                    "Alternativa: defínela en la variable de entorno {}.",
                    tab.api_key_env()
                ));
                // Los modelos se muestran tal como los devuelve el proveedor
                // (detección con la clave), nunca una lista fija (HU-24).
                match detected.get(&tab) {
                    Some(models) => ui.small(format!(
                        "Modelos detectados ({}): {}",
                        models.len(),
                        models.join(", ")
                    )),
                    None if detecting.contains(&tab) => ui.small("Detectando modelos…"),
                    None => ui.small(
                        "Guarda la clave para detectar automáticamente los modelos disponibles.",
                    ),
                };
            });

        self.key_tab = tab;

        for action in actions {
            match action {
                KeyAction::Save(provider) => self.save_key(provider),
                KeyAction::Delete(provider) => self.delete_key(provider),
                KeyAction::Test(provider) => self.test_key(provider),
            }
        }

        if !open {
            self.keys_open = false;
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }

        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        let mut theme_change: Option<Theme> = None;
        let mut pick_directory = false;
        let mut discard_directory = false;
        let mut apply_directory: Option<bool> = None;
        let mut export = false;
        let mut import = false;
        let mut check_tools = false;
        let mut clear_warnings = false;

        let theme = self.config.theme;
        let current_dir = self.paths.data_dir.clone();
        let draft_dir = self.data_dir_draft.clone();
        let warnings = self.settings_draft.warnings();

        egui::Window::new("Configuración")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(660.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(560.0)
                    .show(ui, |ui| {
                ui.strong("Apariencia");
                ui.horizontal(|ui| {
                    ui.label("Tema:");
                    let mut selected = theme;
                    egui::ComboBox::from_id_salt("theme_selector")
                        .selected_text(theme_label(theme))
                        .show_ui(ui, |ui| {
                            for candidate in [Theme::System, Theme::Light, Theme::Dark] {
                                ui.selectable_value(
                                    &mut selected,
                                    candidate,
                                    theme_label(candidate),
                                );
                            }
                        });
                    if selected != theme {
                        theme_change = Some(selected);
                    }
                });

                ui.add_space(8.0);
                ui.separator();
                ui.strong("Parámetros del RAG");
                let draft = &mut self.settings_draft;
                egui::Grid::new("settings_grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Tamaño de fragmento (caracteres)");
                        ui.add(
                            egui::DragValue::new(&mut draft.chunk_size)
                                .range(raggy_core::MIN_CHUNK_SIZE..=raggy_core::MAX_CHUNK_SIZE),
                        );
                        ui.end_row();

                        ui.label("Solapamiento (caracteres)");
                        ui.add(
                            egui::DragValue::new(&mut draft.chunk_overlap)
                                .range(0..=raggy_core::MAX_CHUNK_OVERLAP),
                        );
                        ui.end_row();

                        ui.label("Fragmentos recuperados (top-k)");
                        ui.add(
                            egui::DragValue::new(&mut draft.top_k)
                                .range(1..=raggy_core::MAX_TOP_K),
                        );
                        ui.end_row();

                        ui.label("Corte relativo del contexto")
                            .on_hover_text(
                                "Fracción del mejor resultado que debe alcanzar un fragmento \
                                 para usarse como contexto. Bajo = más contexto; alto = más \
                                 estricto y puede dejar la respuesta sin fuentes.",
                            );
                        ui.add(
                            egui::DragValue::new(&mut draft.similarity_threshold)
                                .range(
                                    raggy_core::MIN_SIMILARITY_THRESHOLD
                                        ..=raggy_core::MAX_SIMILARITY_THRESHOLD,
                                )
                                .speed(0.01),
                        );
                        ui.end_row();

                        ui.label("Dimensionalidad del embedding");
                        ui.add(
                            egui::DragValue::new(&mut draft.embedding_dimensions).range(
                                raggy_core::MIN_EMBEDDING_DIMENSIONS
                                    ..=raggy_core::MAX_EMBEDDING_DIMENSIONS,
                            ),
                        );
                        ui.end_row();
                    });

                ui.add_space(6.0);
                ui.small("Cambiar el tamaño de fragmento o la dimensionalidad requiere reindexar.");
                for warning in &warnings {
                    ui.colored_label(
                        crate::theme::warning_color(ui.visuals().dark_mode),
                        format!("Aviso: {warning}"),
                    );
                }

                ui.add_space(8.0);
                ui.separator();
                ui.strong("OCR y conversión de documentos");
                ui.checkbox(
                    &mut self.settings_ingest.ocr_enabled,
                    "Aplicar OCR a los PDF escaneados cuando esté disponible",
                );
                ui.horizontal(|ui| {
                    ui.label("Idiomas de OCR");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings_languages)
                            .desired_width(160.0)
                            .hint_text("spa"),
                    );
                    ui.small("separados por coma (p. ej. spa, eng)");
                });
                ui.horizontal(|ui| {
                    ui.label("Texto mínimo por página para no usar OCR");
                    ui.add(
                        egui::DragValue::new(&mut self.settings_ingest.min_text_chars)
                            .range(0..=raggy_core::ingest_limits::MAX_MIN_TEXT_CHARS),
                    );
                });
                egui::Grid::new("ingest_limits_grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Tamaño máximo de archivo (MiB)");
                        ui.add(
                            egui::DragValue::new(&mut self.settings_ingest.max_file_mb)
                                .range(1..=raggy_core::ingest_limits::MAX_MAX_FILE_MIB),
                        );
                        ui.end_row();

                        ui.label("Descompresión máxima Office (MiB)");
                        ui.add(egui::DragValue::new(
                            &mut self.settings_ingest.max_archive_mb,
                        ).range(1..=raggy_core::ingest_limits::MAX_MAX_ARCHIVE_MIB));
                        ui.end_row();

                        ui.label("Máximo de páginas PDF");
                        ui.add(
                            egui::DragValue::new(&mut self.settings_ingest.max_pages)
                                .range(1..=raggy_core::ingest_limits::MAX_MAX_PAGES),
                        );
                        ui.end_row();

                        ui.label("Máximo de texto por página PDF (MiB)");
                        ui.add(
                            egui::DragValue::new(
                                &mut self.settings_ingest.max_pdf_page_text_mb,
                            )
                            .range(1..=raggy_core::ingest_limits::MAX_MAX_PDF_PAGE_TEXT_MIB),
                        );
                        ui.end_row();

                        ui.label("Máximo de texto por documento PDF (MiB)");
                        ui.add(
                            egui::DragValue::new(&mut self.settings_ingest.max_pdf_text_mb)
                                .range(1..=raggy_core::ingest_limits::MAX_MAX_PDF_TEXT_MIB),
                        );
                        ui.end_row();

                        ui.label("Tiempo máximo por herramienta (segundos)");
                        ui.add(egui::DragValue::new(
                            &mut self.settings_ingest.process_timeout_secs,
                        ).range(1..=raggy_core::ingest_limits::MAX_PROCESS_TIMEOUT_SECS));
                        ui.end_row();

                        ui.label("Píxeles máximos por página OCR");
                        ui.add(egui::DragValue::new(
                            &mut self.settings_ingest.max_ocr_image_pixels,
                        ).range(
                            raggy_core::ingest_limits::MIN_MAX_OCR_IMAGE_PIXELS
                                ..=raggy_core::ingest_limits::MAX_MAX_OCR_IMAGE_PIXELS,
                        ));
                        ui.end_row();
                    });
                ui.horizontal(|ui| {
                    if ui.button("Comprobar herramientas").clicked() {
                        check_tools = true;
                    }
                    if !self.index_warnings.is_empty() && ui.button("Limpiar avisos").clicked() {
                        clear_warnings = true;
                    }
                });
                if let Some(report) = &self.tools_report {
                    ui.add_space(4.0);
                    ui.monospace(report);
                }
                if !self.index_warnings.is_empty() {
                    ui.add_space(4.0);
                    ui.strong("Avisos de la última indexación");
                    for warning in &self.index_warnings {
                        ui.label(format!("• {warning}"));
                    }
                }

                ui.add_space(8.0);
                ui.separator();
                ui.strong("Directorio de datos");
                ui.small(format!(
                    "Configuración: {}",
                    self.paths.settings_path().display()
                ));
                ui.small(format!("Datos: {}", current_dir.display()));
                ui.horizontal(|ui| {
                    if ui.button("Cambiar carpeta…").clicked() {
                        pick_directory = true;
                    }
                    match &draft_dir {
                        Some(directory) => {
                            ui.monospace(directory.display().to_string());
                        }
                        None => {
                            ui.small("Sin cambios pendientes");
                        }
                    }
                });
                if draft_dir.is_some() {
                    ui.horizontal(|ui| {
                        if ui.button("Migrar datos y aplicar").clicked() {
                            apply_directory = Some(true);
                        }
                        if ui.button("Aplicar sin migrar").clicked() {
                            apply_directory = Some(false);
                        }
                        if ui.button("Descartar").clicked() {
                            discard_directory = true;
                        }
                    });
                    ui.small(
                        "El cambio se aplicará al reiniciar la aplicación. Migrar copia la base de \
                         datos, la configuración, los índices y los artefactos.",
                    );
                }

                ui.add_space(8.0);
                ui.separator();
                ui.strong("Respaldo");
                ui.horizontal(|ui| {
                    if ui.button("Exportar todo").clicked() {
                        export = true;
                    }
                    if ui.button("Importar respaldo…").clicked() {
                        import = true;
                    }
                });
                ui.small(
                    "Incluye base de datos, configuración sin claves, archivos subidos y \
                     artefactos físicos registrados. Se valida el ZIP antes de restaurar \
                     en un directorio nuevo.",
                );

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Guardar").clicked() {
                        save = true;
                    }
                    if ui.button("Cerrar").clicked() {
                        cancel = true;
                    }
                });
                    });
            });

        if let Some(theme) = theme_change {
            self.config.theme = theme;
            ctx.set_theme(crate::theme::preference(theme));
            if let Err(error) = self.config.save(&self.paths.settings_path()) {
                tracing::warn!("no se pudo guardar el tema: {error}");
            }
        }
        if cancel || !open {
            self.settings_draft = self.config.rag.clone();
            self.settings_ingest = self.config.ingest.clone();
            self.settings_languages = self.config.ingest.ocr_languages.join(", ");
            self.settings_open = false;
        }
        if pick_directory {
            let mut dialog = rfd::FileDialog::new().set_title("Elige el directorio de datos");
            dialog = dialog.set_directory(&current_dir);
            if let Some(directory) = dialog.pick_folder() {
                self.data_dir_draft = Some(directory);
            }
        }
        if discard_directory {
            self.data_dir_draft = None;
        }
        if let Some(migrate) = apply_directory {
            self.apply_data_dir(migrate);
        }
        if export {
            self.export_backup_now();
        }
        if import {
            self.import_backup_pick();
        }
        if check_tools {
            self.settings_ingest.ocr_languages = parse_languages(&self.settings_languages);
            self.tools_report = Some(tools_report(
                &self.settings_ingest.ocr_languages,
                self.settings_ingest.process_timeout_secs,
            ));
        }
        if clear_warnings {
            self.index_warnings.clear();
        }

        if save {
            let mut candidate = self.config.clone();
            candidate.rag = self.settings_draft.clone();
            candidate.ingest = self.settings_ingest.clone();
            candidate.ingest.ocr_languages = parse_languages(&self.settings_languages);
            match candidate.validate() {
                Ok(()) => {
                    for secret in self.config.api_keys.values_mut() {
                        secret.zeroize();
                    }
                    self.config = candidate;
                    match self.config.save(&self.paths.settings_path()) {
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
        }
    }

    /// Aplica el nuevo directorio de datos, migrando o no (HU-28).
    ///
    /// El directorio se guarda en `settings.json` (`data_dir`) y se aplica al
    /// reiniciar.
    fn apply_data_dir(&mut self, migrate: bool) {
        let Some(directory) = self.data_dir_draft.clone() else {
            return;
        };
        let target = AppPaths::with_data_dir(&self.paths.home_dir, &directory);
        if target.data_dir == self.paths.data_dir {
            self.data_dir_draft = None;
            return;
        }

        if migrate {
            let outcome = self
                .storage
                .as_ref()
                .map(|storage| raggy_core::migrate_data(&self.paths, &target, storage));

            match outcome {
                Some(Ok(count)) => {
                    self.status = format!(
                        "Migrados {count} archivos a {}. Reinicia la aplicación para usar la nueva ubicación.",
                        directory.display()
                    );
                }
                Some(Err(error)) => {
                    self.status = format!("No se pudo migrar: {error}");
                    return;
                }
                None => {
                    self.status = "La base de datos no está disponible.".to_string();
                    return;
                }
            }
        } else {
            self.status = format!(
                "Se usará {} al reiniciar; los datos actuales no se migran.",
                directory.display()
            );
        }

        // El directorio de datos se recuerda en settings.json.
        self.config.data_dir = Some(directory);
        if let Err(error) = self.config.save(&self.paths.settings_path()) {
            self.status = format!("No se pudo guardar settings.json: {error}");
            return;
        }
        self.data_dir_draft = None;
    }

    /// Exporta el respaldo completo (HU-32).
    fn export_backup_now(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Exportar respaldo")
            .set_file_name("raggy-respaldo.zip");
        if let Some(directory) = &self.last_download_dir {
            dialog = dialog.set_directory(directory);
        }
        let Some(path) = dialog.save_file() else {
            return;
        };

        let outcome = self
            .storage
            .as_ref()
            .map(|storage| raggy_core::export_backup(storage, &self.paths, &path));

        match outcome {
            Some(Ok(summary)) => {
                self.status = format!(
                    "Respaldo exportado a {} ({} archivos, {} KiB).",
                    path.display(),
                    summary.files,
                    summary.bytes / 1024
                );
            }
            Some(Err(error)) => self.status = format!("No se pudo exportar el respaldo: {error}"),
            None => self.status = "La base de datos no está disponible.".to_string(),
        }
    }

    /// Pide un archivo de respaldo para importarlo (HU-32).
    fn import_backup_pick(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Selecciona un respaldo")
            .add_filter("Respaldo de Raggy", &["zip"]);
        if let Some(directory) = &self.last_download_dir {
            dialog = dialog.set_directory(directory);
        }
        if let Some(path) = dialog.pick_file() {
            self.pending_import = Some(path);
        }
    }

    fn import_backup_now(&mut self) {
        let Some(archive) = self.pending_import.take() else {
            return;
        };
        match raggy_core::import_backup(&archive, &self.paths) {
            Ok(summary) => {
                if let Some(restored_dir) = summary.restored_data_dir {
                    self.config = AppConfig::load_or_default(&self.paths.settings_path());
                    self.settings_draft = self.config.rag.clone();
                    self.settings_ingest = self.config.ingest.clone();
                    self.settings_languages = self.config.ingest.ocr_languages.join(", ");
                    self.refresh_keys();
                    self.status = format!(
                        "Respaldo restaurado ({} archivos) en {}. Reinicia la aplicación para usarlo.",
                        summary.files,
                        restored_dir.display()
                    );
                } else {
                    self.status = format!(
                        "Respaldo restaurado ({} archivos). Reinicia la aplicación para usar los datos.",
                        summary.files
                    );
                }
            }
            Err(error) => self.status = format!("No se pudo importar el respaldo: {error}"),
        }
    }

    fn import_confirm(&mut self, ctx: &egui::Context) {
        let Some(archive) = self.pending_import.clone() else {
            return;
        };

        let mut confirm = false;
        let mut cancel = false;

        egui::Window::new("Restaurar respaldo")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!("¿Restaurar «{}»?", archive.display()));
                ui.small(
                    "Se preparará un directorio de datos nuevo y se cambiará la \
                     configuración al restaurarlo. El directorio actual se conserva. \
                     Reinicia la aplicación después de restaurar.",
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancelar").clicked() {
                        cancel = true;
                    }
                    if ui.button("Restaurar").clicked() {
                        confirm = true;
                    }
                });
            });

        if confirm {
            self.import_backup_now();
        } else if cancel {
            self.pending_import = None;
        }
    }

    /// Aviso de primera ejecución sobre qué se envía y a dónde (HU-30).
    fn notice_window(&mut self, ctx: &egui::Context) {
        if self.config.privacy_notice_version >= raggy_core::PRIVACY_NOTICE_VERSION {
            return;
        }

        let mut accept = false;

        egui::Window::new("Privacidad de tus datos")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(
                    "Raggy funciona en tu equipo: la indexación y las búsquedas se hacen en local \
                     con embeddings propios.",
                );
                ui.add_space(4.0);
                ui.label(
                    "Al usar el chat, se envían al proveedor que elijas tu pregunta, los \
                     fragmentos recuperados y el historial previo de la conversación. Si pides \
                     modificar un artefacto, también puede enviarse su contenido. No hay telemetría.",
                );
                ui.add_space(4.0);
                ui.label(
                    "Las claves de API se guardan en texto claro en settings.json (permisos \
                     restringidos en Unix); no se cifran ni se guardan en el llavero del sistema.",
                );
                ui.add_space(8.0);
                if ui.button("Entendido").clicked() {
                    accept = true;
                }
            });

        if accept {
            self.config.privacy_notice_version = raggy_core::PRIVACY_NOTICE_VERSION;
            if let Err(error) = self.config.save(&self.paths.settings_path()) {
                tracing::warn!("no se pudo guardar el aviso de privacidad: {error}");
            }
        }
    }

    /// Ventana flotante que avisa de que al proveedor le falta la API Key (HU-23).
    ///
    /// Sustituye al mensaje fijo «Sin API Key (… )» de la cabecera: aparece como
    /// alerta, explica la variable de entorno y lleva a configurarla.
    fn api_key_alert_window(&mut self, ctx: &egui::Context) {
        let Some(provider) = self.api_key_alert else {
            return;
        };

        let mut configure = false;
        let mut close = false;

        egui::Window::new("Falta la API Key")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_max_width(460.0);
                ui.label(format!(
                    "El proveedor {} no tiene una API Key configurada, así que no se puede \
                     generar la respuesta.",
                    provider.display_name()
                ));
                ui.add_space(4.0);
                ui.label(format!(
                    "Guárdala en Claves de API o define la variable de entorno {}.",
                    provider.api_key_env()
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Configurar claves").clicked() {
                        configure = true;
                    }
                    if ui.button("Cerrar").clicked() {
                        close = true;
                    }
                });
            });

        if close || ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.api_key_alert = None;
        }
        if configure {
            self.api_key_alert = None;
            self.key_tab = provider;
            self.keys_open = true;
        }
    }

    /// Exporta la sesión activa a Markdown (HU-31).
    fn export_session(&mut self) {
        let Some(session) = self.active_session.clone() else {
            return;
        };
        let folder_name = self
            .selected_folder
            .as_ref()
            .and_then(|id| self.entries.iter().find(|entry| &entry.folder.id == id))
            .map(|entry| entry.folder.name.clone())
            .unwrap_or_else(|| "carpeta".to_string());

        let mut dialog = rfd::FileDialog::new().set_title("Exportar sesión a Markdown");
        if let Some(directory) = &self.last_download_dir {
            dialog = dialog.set_directory(directory);
        }
        let Some(directory) = dialog.pick_folder() else {
            return;
        };
        self.last_download_dir = Some(directory.clone());

        let messages = self.messages.clone();
        let artifacts = self.artifacts.clone();
        match raggy_core::write_session_export(
            &directory,
            &session,
            &folder_name,
            &messages,
            &artifacts,
        ) {
            Ok(path) => self.status = format!("Sesión exportada a {}.", path.display()),
            Err(error) => self.status = format!("No se pudo exportar la sesión: {error}"),
        }
    }

    fn handle_actions(&mut self, ui: &mut egui::Ui) {
        let actions = Self::top_bar(ui, &self.paths.data_dir);
        if actions.add {
            self.add_folder();
        }
        if actions.upload {
            self.upload_files();
        }
        if actions.reindex_all {
            self.reindex_all();
        }
        if actions.settings {
            self.settings_draft = self.config.rag.clone();
            self.settings_open = true;
        }
        if actions.keys {
            self.refresh_keys();
            self.keys_open = true;
        }
        if actions.artifacts {
            self.refresh_artifacts();
            self.artifact_scope = ArtifactScope::Session;
            self.artifacts_open = true;
        }

        let sidebar = Self::sidebar(
            ui,
            &self.entries,
            self.selected_folder.as_deref(),
            &self.sessions,
            self.active_session
                .as_ref()
                .map(|session| session.id.as_str()),
        );
        if let Some(id) = sidebar.select_folder {
            self.select_folder(id);
        }
        if let Some(id) = sidebar.reindex {
            self.reindex_folder(&id);
        }
        if let Some(id) = sidebar.delete_folder {
            self.pending_delete = Some(id);
        }
        if sidebar.new_session {
            self.new_session();
        }
        if let Some(id) = sidebar.select_session {
            self.select_session(id);
        }
        if let Some(id) = sidebar.rename_session {
            let title = self
                .sessions
                .iter()
                .find(|session| session.id == id)
                .map(|session| session.title.clone())
                .unwrap_or_default();
            self.session_dialog = Some(SessionDialog::Rename { id, title });
        }
        if let Some(id) = sidebar.delete_session {
            self.session_dialog = Some(SessionDialog::Delete { id });
        }

        // El panel de citas se declara dentro de cada vista (chat o exploración),
        // debajo de su barra de botones, para no recortarla ni taparla.
        Self::status_bar(ui, &self.status);
        for action in self.central(ui) {
            match action {
                CentralAction::Search => self.run_search(),
                CentralAction::ClearSearch => {
                    self.results.clear();
                    self.search_note = None;
                }
                CentralAction::OpenDocument(path) => self.load_chunks(&path),
                CentralAction::OpenCitation(citation) => self.open_citation(&citation),
                CentralAction::SendMessage => self.send_message(),
                CentralAction::StopGeneration => {
                    if let Some(worker) = &self.chat_worker {
                        worker.cancel();
                    }
                }
                CentralAction::UpdateModel { provider, model } => {
                    self.update_session_model(provider, model);
                }
                CentralAction::UpdateFallback { provider } => {
                    self.set_session_fallback(provider);
                }
                CentralAction::RetryWithFallback => self.retry_with_fallback(),
                CentralAction::AttachFiles => self.attach_files(),
                CentralAction::RemoveAttachment(relative_path) => {
                    self.remove_attachment(&relative_path);
                }
                CentralAction::CopyCode(text) => {
                    ui.ctx().copy_text(text);
                    self.status = "Código copiado al portapapeles.".to_string();
                }
                CentralAction::DownloadBlock {
                    message_id,
                    block_index,
                } => self.download_block(message_id, block_index),
                CentralAction::DownloadAll { message_id } => self.start_download_all(message_id),
                CentralAction::ExportSession => self.export_session(),
            }
        }
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

        match self
            .search_cache
            .search_folder(storage, &folder_id, &query, &options, dimensions)
        {
            Ok(results) => {
                self.search_note = Some(if results.is_empty() {
                    format!(
                        "No se encontró ningún fragmento relacionado (corte relativo {:.2}).",
                        options.relative_floor
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
}

impl eframe::App for RaggyApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Aplica la paleta cuando cambia el tema elegido o el del sistema.
        let resolved = ctx.theme();
        let key = (self.config.theme, resolved == egui::Theme::Dark);
        if self.visuals_applied != Some(key) {
            crate::theme::apply_style(&ctx, self.config.theme);
            self.visuals_applied = Some(key);
        }

        // Recuerda el tamaño de la ventana para restaurarlo en el próximo arranque.
        let size = ui.max_rect().size();
        if size.x > 1.0 && size.y > 1.0 {
            self.config.window_size = Some([size.x, size.y]);
        }

        self.drain_events();
        self.drain_chat_events();
        self.drain_key_tests();
        self.drain_model_detections();
        self.poll_watcher();
        if self.worker.is_some() || self.chat_worker.is_some() || self.watcher.is_some() {
            ctx.request_repaint_after(Duration::from_millis(120));
        }

        self.handle_actions(ui);
        self.delete_dialog(&ctx);
        self.session_dialog_window(&ctx);
        self.settings_window(&ctx);
        self.keys_window(&ctx);
        self.artifacts_window(&ctx);
        self.batch_dialog(&ctx);
        self.conflict_dialog(&ctx);
        self.import_confirm(&ctx);
        self.notice_window(&ctx);
        self.api_key_alert_window(&ctx);
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        if let Err(error) = self.config.save(&self.paths.settings_path()) {
            tracing::warn!("no se pudo guardar la configuración al cerrar: {error}");
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Err(error) = self.config.save(&self.paths.settings_path()) {
            tracing::warn!("no se pudo guardar la configuración al salir: {error}");
        }
        self.wipe_secret_copies();
    }
}

/// Abrevia una marca de tiempo RFC 3339 a fecha y hora.
fn short_timestamp(value: &str) -> String {
    value
        .get(..16)
        .map(|slice| slice.replace('T', " "))
        .unwrap_or_else(|| value.to_string())
}

/// Tamaño legible de un archivo adjunto (bytes, KB o MB).
fn human_size(bytes: i64) -> String {
    const KIB: f64 = 1024.0;
    let bytes = bytes.max(0) as f64;
    if bytes < KIB {
        format!("{bytes:.0} B")
    } else if bytes < KIB * KIB {
        format!("{:.0} KB", bytes / KIB)
    } else {
        format!("{:.1} MB", bytes / (KIB * KIB))
    }
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

/// Etiqueta legible de un tema.
/// Convierte una lista de idiomas separada por comas en un vector limpio.
fn parse_languages(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|language| !language.is_empty())
        .map(str::to_string)
        .collect()
}

fn theme_label(theme: Theme) -> &'static str {
    match theme {
        Theme::Light => "Claro · Novel",
        Theme::Dark => "Oscuro · Tokyo Night",
        Theme::System => "Sistema",
    }
}

/// Texto descriptivo del estado de una clave de API (HU-23, HU-24).
fn key_status_text(status: Option<&KeyStatus>) -> String {
    match status {
        Some(KeyStatus::Stored { masked }) => format!("Guardada · {masked}"),
        Some(KeyStatus::Environment { env_var }) => format!("Desde la variable {env_var}"),
        Some(KeyStatus::Testing) => "Comprobando…".to_string(),
        Some(KeyStatus::Ok { count, sample }) if sample.is_empty() => {
            format!("Conexión correcta · {count} modelos")
        }
        Some(KeyStatus::Ok { count, sample }) => {
            format!("Conexión correcta · {count} modelos (p. ej. {sample})")
        }
        Some(KeyStatus::Error(message)) => format!("Error: {message}"),
        Some(KeyStatus::Missing) | None => "Sin configurar".to_string(),
    }
}

/// Dibuja un mensaje como burbuja; a la derecha si es del usuario.
///
/// El texto del asistente se interpreta como Markdown.
fn message_bubble(ui: &mut egui::Ui, content: &str, is_user: bool) {
    let dark = ui.visuals().dark_mode;
    let fill = if is_user {
        crate::theme::user_bubble(dark)
    } else {
        crate::theme::assistant_bubble(dark)
    };

    let full = ui.available_width();
    let bubble = (full * 0.78).clamp(240.0, full.max(240.0));
    let inner = (bubble - 30.0).max(180.0);

    ui.horizontal(|ui| {
        if is_user {
            ui.add_space((full - bubble).max(0.0));
        }

        egui::Frame::NONE
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, crate::theme::bubble_stroke(dark)))
            .inner_margin(egui::Margin::symmetric(14, 10))
            .corner_radius(egui::CornerRadius::same(10))
            .show(ui, |ui| {
                ui.set_width(inner);
                // Vertical: si no, cada bloque de Markdown se colocaría a la
                // derecha del anterior (el frame hereda el layout horizontal).
                ui.vertical(|ui| {
                    ui.set_width(inner);
                    if is_user {
                        ui.add(egui::Label::new(content).wrap());
                    } else {
                        crate::markdown::render(ui, content, inner);
                    }
                });
            });
    });
}

/// Dibuja una fila tarjeta del panel lateral (carpeta o sesión).
fn card(ui: &mut egui::Ui, highlighted: bool, add_contents: impl FnOnce(&mut egui::Ui)) {
    let dark = ui.visuals().dark_mode;
    let fill = if highlighted {
        crate::theme::selection_fill(dark)
    } else {
        egui::Color32::TRANSPARENT
    };

    ui.add_space(2.0);
    egui::Frame::NONE
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.set_width((ui.available_width() - 4.0).max(120.0));
            ui.vertical(|ui| {
                ui.set_width((ui.available_width()).max(120.0));
                add_contents(ui);
            });
        });
    ui.add_space(2.0);
}

/// Título de sección del panel lateral.
fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(egui::RichText::new(text).small().strong());
    });
}

/// Texto de ayuda con sangría.
fn hint(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.weak(egui::RichText::new(text).small());
    });
}

/// Punto de color para indicar el estado.
fn status_dot(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
    ui.add_space(2.0);
}

/// Color del contenido de una tarjeta según si está resaltada.
fn tint_for(ui: &egui::Ui, highlighted: bool) -> egui::Color32 {
    if highlighted {
        crate::theme::accent(ui.visuals().dark_mode)
    } else {
        ui.visuals().weak_text_color()
    }
}

/// Dibuja los bloques de código de un mensaje con nombre editable y acciones.
#[allow(clippy::too_many_arguments)]
fn render_code_blocks(
    ui: &mut egui::Ui,
    message_id: &str,
    blocks: &[CodeBlock],
    edits: &HashMap<String, String>,
    previews: &HashMap<String, bool>,
    pending_edits: &mut Vec<(String, String)>,
    pending_previews: &mut Vec<String>,
    actions: &mut Vec<CentralAction>,
) {
    if blocks.is_empty() {
        return;
    }

    if blocks.len() > 1
        && ui
            .small_button(format!("Descargar todo ({})", blocks.len()))
            .clicked()
    {
        actions.push(CentralAction::DownloadAll {
            message_id: message_id.to_string(),
        });
    }

    for (index, block) in blocks.iter().enumerate() {
        let key = format!("{message_id}:{index}");
        let dark = ui.visuals().dark_mode;
        let expanded = previews.get(&key).copied().unwrap_or(false);

        egui::Frame::NONE
            .fill(crate::theme::code_background(
                dark,
                ui.visuals().extreme_bg_color,
            ))
            .stroke(egui::Stroke::new(1.0, crate::theme::bubble_stroke(dark)))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(6, 6))
            .show(ui, |ui| {
                crate::theme::apply_code_visuals(ui);
                let tint = crate::theme::code_muted(dark, ui.visuals().weak_text_color());
                ui.set_width((ui.available_width() * 0.94).max(240.0));

                ui.horizontal(|ui| {
                    if let Some(language) = &block.language {
                        let lang_color =
                            crate::theme::code_language(dark, ui.visuals().weak_text_color());
                        ui.colored_label(lang_color, language.clone());
                        ui.separator();
                    }

                    let mut name = edits.get(&key).cloned().unwrap_or_else(|| {
                        block
                            .filename
                            .clone()
                            .unwrap_or_else(|| suggest_filename(block.language.as_deref(), index))
                    });
                    let name_color = crate::theme::code_filename(dark, ui.visuals().text_color());
                    let edit = ui
                        .scope(|ui| {
                            ui.visuals_mut().override_text_color = Some(name_color);
                            ui.add(
                                egui::TextEdit::singleline(&mut name)
                                    .desired_width(190.0)
                                    .frame(egui::Frame::NONE),
                            )
                        })
                        .inner;
                    if edit.changed() {
                        pending_edits.push((key.clone(), name.clone()));
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::icons::icon_button(
                            ui,
                            crate::icons::download(),
                            "Descargar",
                            tint,
                        )
                        .clicked()
                        {
                            actions.push(CentralAction::DownloadBlock {
                                message_id: message_id.to_string(),
                                block_index: index,
                            });
                        }
                        if crate::icons::icon_button(ui, crate::icons::copy(), "Copiar", tint)
                            .clicked()
                        {
                            actions.push(CentralAction::CopyCode(block.content.clone()));
                        }
                        let eye_tint = if expanded {
                            crate::theme::accent(dark)
                        } else {
                            tint
                        };
                        if crate::icons::icon_button(
                            ui,
                            crate::icons::eye(),
                            if expanded { "Ver menos" } else { "Ver todo" },
                            eye_tint,
                        )
                        .clicked()
                        {
                            pending_previews.push(key.clone());
                        }
                    });
                });

                ui.separator();

                let max_height = if expanded { 420.0 } else { 148.0 };
                egui::ScrollArea::both()
                    .id_salt(format!("preview_{key}"))
                    .max_height(max_height)
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(highlight_job(
                            ui,
                            block.language.as_deref(),
                            &block.content,
                        )));
                    });
            });
        ui.add_space(4.0);
    }
}

/// ¿La cita y el fragmento se refieren a la misma zona del documento?
///
/// Sirve de respaldo cuando la cita se guardó con otro troceado: los números de
/// línea cambian y la coincidencia exacta falla, pero la ubicación (fila, líneas,
/// página o diapositiva) sigue señalando el mismo lugar.
fn mismo_lugar(citation: &Citation, chunk: &Chunk) -> bool {
    let cited = citation.location.as_deref().and_then(location_range);
    let found = chunk.metadata.location.as_deref().and_then(location_range);
    match (cited, found) {
        (Some((cited_kind, cited_first, cited_last)), Some((kind, first, last))) => {
            cited_kind == kind && cited_first <= last && first <= cited_last
        }
        _ => false,
    }
}

/// Rango numérico de la ubicación de una cita, con su tipo.
///
/// Entiende los formatos que genera la app: `líneas 10-24`, `página 7`,
/// `diapositiva 3`, `hoja Ventas, A2:D20`, `hoja 1, fila 2669` y
/// `hoja 1, filas 2-5`. Devuelve `None` cuando no hay números (p. ej. `hoja 1`).
fn location_range(location: &str) -> Option<(&'static str, usize, usize)> {
    let text = location.trim();
    if let Some(rest) = text.strip_prefix("líneas ") {
        return two_numbers(rest).map(|(first, last)| ("líneas", first, last));
    }
    if let Some(rest) = text.strip_prefix("página ") {
        return one_number(rest).map(|number| ("página", number, number));
    }
    if let Some(rest) = text.strip_prefix("diapositiva ") {
        return one_number(rest).map(|number| ("diapositiva", number, number));
    }

    // Tablas: se toma lo que sigue a la hoja (`hoja Ventas, A2:D20`).
    let cells = text.split_once(',').map_or("", |(_, cells)| cells).trim();
    if let Some(rest) = cells.strip_prefix("fila ") {
        return one_number(rest).map(|row| ("fila", row, row));
    }
    if let Some(rest) = cells.strip_prefix("filas ") {
        return two_numbers(rest).map(|(first, last)| ("fila", first, last));
    }
    let (first, last) = cells.split_once(':')?;
    Some(("fila", one_number(first)?, one_number(last)?))
}

/// Extrae el primer número de un texto (`A2669` → `2669`).
fn one_number(text: &str) -> Option<usize> {
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Extrae dos números separados por un guion (`2-5` → `(2, 5)`).
fn two_numbers(text: &str) -> Option<(usize, usize)> {
    let (first, last) = text.split_once('-')?;
    Some((one_number(first)?, one_number(last)?))
}

/// Categoría de un token para el resaltado de sintaxis.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    Plain,
    Keyword,
    Str,
    Comment,
    Number,
}

struct Token {
    text: String,
    kind: TokenKind,
}

/// Construye el texto resaltado de un bloque de código (HU-18).
fn highlight_job(ui: &egui::Ui, language: Option<&str>, code: &str) -> egui::text::LayoutJob {
    let font = egui::FontId::monospace(13.0);
    let dark = ui.visuals().dark_mode;
    // En modo oscuro, el texto sin clasificar también es el de Monokai.
    let base = crate::theme::code_text(dark, ui.visuals().text_color());
    let mut job = egui::text::LayoutJob::default();

    let width = ui.available_width();
    job.wrap.max_width = if width.is_finite() && width > 0.0 {
        width
    } else {
        900.0
    };

    let palette = crate::theme::syntax_colors(ui.visuals().dark_mode);

    for token in tokenize_code(language, code) {
        let color = match token.kind {
            TokenKind::Plain => base,
            TokenKind::Keyword => palette.keyword,
            TokenKind::Str => palette.string,
            TokenKind::Comment => palette.comment,
            TokenKind::Number => palette.number,
        };
        job.append(
            &token.text,
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color,
                ..Default::default()
            },
        );
    }

    job
}

/// Palabras reservadas reconocidas por el resaltado (varios lenguajes).
const KEYWORDS: &[&str] = &[
    "fn",
    "let",
    "mut",
    "pub",
    "struct",
    "enum",
    "impl",
    "trait",
    "mod",
    "use",
    "crate",
    "self",
    "super",
    "match",
    "loop",
    "while",
    "for",
    "in",
    "if",
    "else",
    "return",
    "break",
    "continue",
    "async",
    "await",
    "move",
    "ref",
    "where",
    "dyn",
    "unsafe",
    "const",
    "static",
    "type",
    "def",
    "class",
    "import",
    "from",
    "as",
    "with",
    "lambda",
    "pass",
    "raise",
    "try",
    "except",
    "finally",
    "yield",
    "global",
    "nonlocal",
    "assert",
    "del",
    "elif",
    "not",
    "and",
    "or",
    "is",
    "None",
    "True",
    "False",
    "function",
    "var",
    "new",
    "this",
    "typeof",
    "instanceof",
    "export",
    "default",
    "extends",
    "package",
    "interface",
    "public",
    "private",
    "protected",
    "void",
    "int",
    "float",
    "double",
    "bool",
    "string",
    "char",
    "null",
    "nil",
    "true",
    "false",
    "func",
    "defer",
    "go",
    "chan",
    "map",
    "range",
    "select",
    "interface",
    "namespace",
    "using",
    "foreach",
    "throws",
    "catch",
    "switch",
    "case",
    "do",
    "then",
    "end",
    "begin",
    "SELECT",
    "FROM",
    "WHERE",
    "INSERT",
    "UPDATE",
    "DELETE",
    "CREATE",
    "TABLE",
    "JOIN",
    "GROUP",
    "ORDER",
    "BY",
];

fn is_keyword(word: &str) -> bool {
    KEYWORDS.contains(&word)
}

/// Marcador de comentario de línea según el lenguaje.
fn line_comment_marker(language: Option<&str>) -> Vec<char> {
    match language.map(str::to_ascii_lowercase).as_deref() {
        Some("python") | Some("py") | Some("bash") | Some("sh") | Some("shell") | Some("zsh")
        | Some("yaml") | Some("yml") | Some("toml") | Some("ruby") | Some("rb") | Some("perl")
        | Some("pl") | Some("r") | Some("makefile") | Some("dockerfile") => vec!['#'],
        Some("sql") | Some("lua") | Some("haskell") => vec!['-', '-'],
        _ => vec!['/', '/'],
    }
}

/// Tokeniza un fragmento de código para el resaltado (HU-18).
fn tokenize_code(language: Option<&str>, code: &str) -> Vec<Token> {
    let marker = line_comment_marker(language);
    let chars: Vec<char> = code.chars().collect();
    let mut tokens = Vec::new();
    let mut plain = String::new();
    let mut index = 0;

    while index < chars.len() {
        let current = chars[index];

        if current == marker[0] && chars[index..].starts_with(&marker[..]) {
            push_plain(&mut tokens, &mut plain);
            let start = index;
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            tokens.push(Token {
                text: chars[start..index].iter().collect(),
                kind: TokenKind::Comment,
            });
            continue;
        }

        if current == '"' || current == '\'' || current == '`' {
            push_plain(&mut tokens, &mut plain);
            let quote = current;
            let start = index;
            index += 1;
            while index < chars.len() {
                if chars[index] == '\\' {
                    index += 2;
                    continue;
                }
                if chars[index] == quote {
                    index += 1;
                    break;
                }
                if chars[index] == '\n' {
                    break;
                }
                index += 1;
            }
            tokens.push(Token {
                text: chars[start..index.min(chars.len())].iter().collect(),
                kind: TokenKind::Str,
            });
            continue;
        }

        if current.is_ascii_digit() {
            push_plain(&mut tokens, &mut plain);
            let start = index;
            while index < chars.len()
                && (chars[index].is_ascii_alphanumeric()
                    || chars[index] == '.'
                    || chars[index] == '_')
            {
                index += 1;
            }
            tokens.push(Token {
                text: chars[start..index].iter().collect(),
                kind: TokenKind::Number,
            });
            continue;
        }

        if current.is_alphabetic() || current == '_' {
            let start = index;
            while index < chars.len() && (chars[index].is_alphanumeric() || chars[index] == '_') {
                index += 1;
            }
            let word: String = chars[start..index].iter().collect();
            if is_keyword(&word) {
                push_plain(&mut tokens, &mut plain);
                tokens.push(Token {
                    text: word,
                    kind: TokenKind::Keyword,
                });
            } else {
                plain.push_str(&word);
            }
            continue;
        }

        plain.push(current);
        index += 1;
    }

    push_plain(&mut tokens, &mut plain);
    tokens
}

fn push_plain(tokens: &mut Vec<Token>, plain: &mut String) {
    if !plain.is_empty() {
        tokens.push(Token {
            text: std::mem::take(plain),
            kind: TokenKind::Plain,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        human_size, location_range, mismo_lugar, short_timestamp, snippet, tokenize_code, Chunk,
        Citation, TokenKind,
    };
    use raggy_rag::ChunkMetadata;

    #[test]
    fn abrevia_marcas_de_tiempo() {
        assert_eq!(
            short_timestamp("2026-10-14T02:15:27.459528Z"),
            "2026-10-14 02:15"
        );
        assert_eq!(short_timestamp("corto"), "corto");
    }

    #[test]
    fn expresa_el_tamano_de_un_adjunto() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2 KB");
        assert_eq!(human_size(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn recorta_el_extracto_a_la_primera_linea() {
        assert_eq!(snippet("\n\n  hola mundo  \nresto"), "hola mundo");
        assert_eq!(snippet("   "), "");
    }

    #[test]
    fn resalta_palabras_reservadas_cadenas_y_comentarios() {
        let tokens = tokenize_code(Some("rust"), "let x = \"hola\"; // nota");
        assert!(tokens
            .iter()
            .any(|token| token.kind == TokenKind::Keyword && token.text == "let"));
        assert!(tokens
            .iter()
            .any(|token| token.kind == TokenKind::Str && token.text == "\"hola\""));
        assert!(tokens
            .iter()
            .any(|token| token.kind == TokenKind::Comment && token.text.contains("nota")));
    }

    #[test]
    fn usa_el_comentario_correcto_segun_el_lenguaje() {
        let python = tokenize_code(Some("python"), "# comentario\nprint(1)");
        assert!(python
            .iter()
            .any(|token| token.kind == TokenKind::Comment && token.text.starts_with("#")));

        let sql = tokenize_code(Some("sql"), "SELECT 1 -- nota");
        assert!(sql
            .iter()
            .any(|token| token.kind == TokenKind::Comment && token.text.starts_with("--")));
    }

    #[test]
    fn reconoce_el_rango_de_una_ubicacion() {
        assert_eq!(location_range("líneas 10-24"), Some(("líneas", 10, 24)));
        assert_eq!(location_range("página 7"), Some(("página", 7, 7)));
        assert_eq!(location_range("diapositiva 3"), Some(("diapositiva", 3, 3)));
        assert_eq!(
            location_range("hoja Ventas, A2:D20"),
            Some(("fila", 2, 20)),
            "un rango de celdas son filas de la 2 a la 20"
        );
        assert_eq!(
            location_range("hoja 1, fila 2669"),
            Some(("fila", 2669, 2669))
        );
        assert_eq!(location_range("hoja 1, filas 2-5"), Some(("fila", 2, 5)));
        assert_eq!(location_range("hoja 1"), None);
    }

    #[test]
    fn localiza_el_fragmento_actual_de_una_cita_antigua() {
        // Cita guardada con el troceado viejo (tres filas) y fragmento nuevo (una).
        let citation = Citation {
            source_file: "ventas.csv".to_string(),
            source_path: "ventas.csv".to_string(),
            heading: None,
            location: Some("hoja 1, A2088:N2090".to_string()),
            start_line: 2090,
            end_line: 2092,
            score: 0.0,
        };
        let chunk = |location: &str| Chunk {
            id: "c".to_string(),
            content_hash: "h".to_string(),
            content: "2090 | V-2025-02089".to_string(),
            metadata: ChunkMetadata {
                source_file: "ventas.csv".to_string(),
                source_path: "ventas.csv".to_string(),
                heading: None,
                location: Some(location.to_string()),
                start_line: 4180,
                end_line: 4180,
            },
        };

        assert!(mismo_lugar(&citation, &chunk("hoja 1, fila 2090")));
        assert!(
            !mismo_lugar(&citation, &chunk("hoja 1, fila 4")),
            "una fila lejana no es la misma zona"
        );
        assert!(
            !mismo_lugar(&citation, &chunk("página 1")),
            "una página no se compara con filas"
        );
    }
}
