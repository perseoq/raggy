//! Aplicación `egui` de Raggy Desktop.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;

use raggy_core::chat::{compose_request, context_from_retrieved};
use raggy_core::index::{IndexEvent, IndexOptions, IndexPhase, IndexWorker};
use raggy_core::watch::FolderWatcher;
use raggy_core::{
    search_folder, AppConfig, AppPaths, ChatEvent, ChatWorker, KeyringStore, ProviderFactory,
    RagConfig, SecretStore, APP_NAME, APP_VERSION,
};
use raggy_llm::ProviderId;
use raggy_rag::{Chunk, ChunkingOptions, RetrievedChunk, RetrieverOptions};
use raggy_storage::{Document, Folder, Message, Session, Storage};

/// Tiempo de espera antes de reindexar tras un cambio detectado en disco.
const REINDEX_DEBOUNCE: Duration = Duration::from_millis(1200);

/// Estado de una clave de API en la interfaz (HU-23, HU-24).
#[derive(Clone)]
enum KeyStatus {
    /// No hay clave configurada.
    Missing,
    /// Clave guardada en el llavero del sistema (enmascarada).
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

/// Diálogo modal de sesiones.
enum SessionDialog {
    Rename { id: String, title: String },
    Delete { id: String },
}

/// Acciones solicitadas desde la barra superior.
#[derive(Default)]
struct TopActions {
    add: bool,
    reindex_all: bool,
    settings: bool,
    keys: bool,
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
    SendMessage,
    StopGeneration,
    UpdateModel { provider: ProviderId, model: String },
    UpdateFallback { provider: Option<ProviderId> },
    RetryWithFallback,
    OpenKeys,
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
    documents: Vec<Document>,
    chunks_preview: Vec<Chunk>,
    pending_delete: Option<String>,
    last_watch: HashMap<String, Instant>,
    query: String,
    results: Vec<RetrievedChunk>,
    search_note: Option<String>,
    sessions: Vec<Session>,
    active_session: Option<Session>,
    messages: Vec<Message>,
    chat_input: String,
    streaming: Option<StreamingState>,
    chat_error: Option<String>,
    session_dialog: Option<SessionDialog>,
    settings_open: bool,
    settings_draft: RagConfig,
    secrets: Arc<dyn SecretStore>,
    keyring_available: bool,
    keys_open: bool,
    key_inputs: HashMap<ProviderId, String>,
    key_status: HashMap<ProviderId, KeyStatus>,
    test_tx: Sender<(ProviderId, Result<Vec<String>, String>)>,
    test_rx: Receiver<(ProviderId, Result<Vec<String>, String>)>,
    last_failure: Option<Failure>,
    status: String,
    fatal: Option<String>,
}

impl RaggyApp {
    /// Crea la aplicación, abre la base de datos, el indexador y el chat.
    pub fn new(ctx: &eframe::CreationContext<'_>, config: AppConfig, paths: AppPaths) -> Self {
        crate::theme::apply(ctx, config.theme);
        let settings_draft = config.rag.clone();
        let secrets: Arc<dyn SecretStore> = Arc::new(KeyringStore::new());
        let keyring_available = KeyringStore::is_available();
        let (test_tx, test_rx) = mpsc::channel();

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
            documents: Vec::new(),
            chunks_preview: Vec::new(),
            pending_delete: None,
            last_watch: HashMap::new(),
            query: String::new(),
            results: Vec::new(),
            search_note: None,
            sessions: Vec::new(),
            active_session: None,
            messages: Vec::new(),
            chat_input: String::new(),
            streaming: None,
            chat_error: None,
            session_dialog: None,
            settings_open: false,
            settings_draft,
            secrets,
            keyring_available,
            keys_open: false,
            key_inputs: HashMap::new(),
            key_status: HashMap::new(),
            test_tx,
            test_rx,
            last_failure: None,
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
        let factory: ProviderFactory = Arc::new(move |id| {
            let key = factory_secrets
                .get(id)
                .ok()
                .flatten()
                .or_else(|| raggy_llm::api_key(id));
            match key {
                Some(key) => raggy_llm::from_api_key(id, key),
                None => Err(raggy_llm::ProviderError::MissingApiKey {
                    provider: id.display_name(),
                    env_var: id.api_key_env(),
                }),
            }
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
        app
    }

    // --------------------------------------------------------------- claves

    /// Recalcula el estado de las claves de cada proveedor (HU-23).
    fn refresh_keys(&mut self) {
        for provider in ProviderId::ALL {
            let status = match self.secrets.get(provider) {
                Ok(Some(secret)) => KeyStatus::Stored {
                    masked: raggy_core::secrets::mask(&secret),
                },
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
            self.key_status.insert(provider, status);
        }
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

        match self.secrets.set(provider, &secret) {
            Ok(()) => {
                self.key_inputs.remove(&provider);
                self.refresh_keys();
                self.status = format!(
                    "Clave de {} guardada en el llavero.",
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
                self.refresh_keys();
                self.status = format!("Clave de {} eliminada.", provider.display_name());
            }
            Err(error) => {
                self.key_status
                    .insert(provider, KeyStatus::Error(error.to_string()));
            }
        }
    }

    fn test_key(&mut self, provider: ProviderId) {
        let key = self
            .secrets
            .get(provider)
            .ok()
            .flatten()
            .or_else(|| raggy_llm::api_key(provider));

        let Some(key) = key else {
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
            let result = raggy_llm::check_connection_with_key(provider, &key)
                .map_err(|error| error.to_string());
            let _ = sender.send((provider, result));
        });
    }

    fn drain_key_tests(&mut self) {
        for (provider, result) in self.test_rx.try_iter() {
            let status = match result {
                Ok(models) => KeyStatus::Ok {
                    count: models.len(),
                    sample: models.first().cloned().unwrap_or_default(),
                },
                Err(message) => KeyStatus::Error(message),
            };
            self.key_status.insert(provider, status);
        }
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
        }
    }

    fn retriever_options(&self) -> RetrieverOptions {
        RetrieverOptions {
            top_k: self.config.rag.top_k,
            min_score: self.config.rag.similarity_threshold,
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
        self.selected_document = None;
        self.chunks_preview.clear();
        self.results.clear();
        self.search_note = None;
        self.reload_documents();
        self.reload_sessions();
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
                self.selected_folder = Some(session.folder_id.clone());
                self.messages = storage.list_messages(&id).unwrap_or_default();
                self.active_session = Some(session);
                self.chat_input.clear();
                self.chat_error = None;
                self.streaming = None;
                self.results.clear();
                self.search_note = None;
                self.selected_document = None;
                self.chunks_preview.clear();
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
        if self.active_session.as_ref().is_some_and(|s| s.id == id) {
            self.active_session = None;
            self.messages.clear();
            self.streaming = None;
        }
        self.reload_sessions();
        self.status = "Sesión eliminada.".to_string();
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

        // El contexto se recupera solo de la carpeta de la sesión (HU-06).
        let options = self.retriever_options();
        let dimensions = self.config.rag.embedding_dimensions;
        let (blocks, citations) = match &self.storage {
            Some(storage) => {
                match search_folder(storage, &session.folder_id, &prompt, &options, dimensions) {
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
            &history,
            &prompt,
        );

        if persist_user {
            if let Some(storage) = &self.storage {
                let user_message = Message::new(&session.id, "user", &prompt, true, Vec::new());
                if let Err(error) = storage.add_message(&user_message) {
                    self.chat_error = Some(format!("No se pudo guardar el mensaje: {error}"));
                    return;
                }
                let _ = storage.touch_session(&session.id);
                self.messages = storage.list_messages(&session.id).unwrap_or_default();
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

    // ----------------------------------------------------------- indexación

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
        if self.selected_folder.as_deref() == Some(id) {
            self.selected_folder = None;
            self.active_session = None;
            self.sessions.clear();
            self.messages.clear();
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
                ui.label(format!("v{APP_VERSION} · Fase 4"));
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
                if ui.button("Claves de API").clicked() {
                    actions.keys = true;
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
        selected_folder: Option<&str>,
        sessions: &[Session],
        active_session: Option<&str>,
    ) -> SidebarActions {
        let mut actions = SidebarActions::default();
        egui::Panel::left("sidebar")
            .default_size(320.0)
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.strong("Carpetas indexadas");
                ui.add_space(6.0);

                egui::ScrollArea::vertical()
                    .id_salt("sidebar")
                    .show(ui, |ui| {
                        if entries.is_empty() {
                            ui.label("Aún no hay carpetas. Usa «Añadir carpeta».");
                        }

                        for entry in entries {
                            let is_selected = selected_folder == Some(entry.folder.id.as_str());
                            if ui
                                .selectable_label(is_selected, &entry.folder.name)
                                .clicked()
                            {
                                actions.select_folder = Some(entry.folder.id.clone());
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
                                    actions.delete_folder = Some(entry.folder.id.clone());
                                }
                            });
                            ui.add_space(8.0);
                        }

                        if selected_folder.is_some() {
                            ui.separator();
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.strong("Sesiones");
                                if ui.small_button("Nueva").clicked() {
                                    actions.new_session = true;
                                }
                            });
                            ui.add_space(4.0);

                            if sessions.is_empty() {
                                ui.small("Sin sesiones en esta carpeta.");
                            }
                            for session in sessions {
                                let is_active = active_session == Some(session.id.as_str());
                                if ui.selectable_label(is_active, &session.title).clicked() {
                                    actions.select_session = Some(session.id.clone());
                                }
                                ui.small(format!("{} · {}", session.provider, session.model));
                                ui.horizontal(|ui| {
                                    if ui.small_button("Renombrar").clicked() {
                                        actions.rename_session = Some(session.id.clone());
                                    }
                                    if ui.small_button("Eliminar").clicked() {
                                        actions.delete_session = Some(session.id.clone());
                                    }
                                });
                                ui.add_space(6.0);
                            }
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

            if self.active_session.is_some() {
                actions.extend(self.chat_view(ui));
                return;
            }

            if self.selected_folder.is_none() {
                ui.vertical_centered(|ui| {
                    ui.add_space(96.0);
                    ui.heading(APP_NAME);
                    ui.add_space(8.0);
                    ui.label("Añade una carpeta con archivos Markdown para indexarla.");
                    ui.add_space(16.0);
                    ui.label("Después podrás buscar en su contenido o iniciar una sesión de chat.");
                });
                return;
            }

            actions.extend(self.explore_view(ui));
        });
        actions
    }

    /// Vista de exploración: búsqueda y archivos de la carpeta (Fase 1 y 2).
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
                            actions.push(CentralAction::OpenDocument(metadata.source_path.clone()));
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
                            ui.label(document.chunk_count.to_string());
                            ui.label(document.size_bytes.to_string());
                            ui.label(short_timestamp(&document.indexed_at));
                            ui.end_row();
                        }
                    });
            });

        actions
    }

    /// Vista de chat de la sesión activa (Fase 3).
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

        ui.add_space(4.0);
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
            egui::ComboBox::from_id_salt("model_selector")
                .selected_text(session.model.clone())
                .show_ui(ui, |ui| {
                    for candidate in selected_provider.models() {
                        ui.selectable_value(
                            &mut selected_model,
                            (*candidate).to_string(),
                            *candidate,
                        );
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
                ui.colored_label(
                    egui::Color32::from_rgb(200, 140, 0),
                    format!("Sin API Key ({})", provider.api_key_env()),
                );
                if ui.small_button("Configurar").clicked() {
                    actions.push(CentralAction::OpenKeys);
                }
            }
        });
        ui.separator();

        egui::ScrollArea::vertical()
            .id_salt("chat_messages")
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.messages.is_empty() {
                    ui.label("Sin mensajes todavía. Escribe abajo para empezar.");
                }

                for message in &self.messages {
                    let is_user = message.role == "user";
                    ui.horizontal(|ui| {
                        ui.strong(if is_user { "Tú" } else { "Asistente" });
                        if !is_user && !message.anchored {
                            ui.small("(no anclada)");
                        }
                    });
                    ui.add(egui::Label::new(&message.content).wrap());

                    if !message.citations.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.small("Fuentes:");
                            for citation in &message.citations {
                                if ui.link(citation.label()).clicked() {
                                    actions.push(CentralAction::OpenDocument(
                                        citation.source_path.clone(),
                                    ));
                                }
                            }
                        });
                    }
                    ui.separator();
                }

                if let Some(text) = &streaming_text {
                    ui.strong("Asistente");
                    ui.add(egui::Label::new(text).wrap());
                    ui.small("generando…");
                }
            });

        ui.separator();
        let mut input = std::mem::take(&mut self.chat_input);
        ui.add_enabled_ui(!is_streaming, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut input)
                    .desired_rows(3)
                    .hint_text("Escribe tu pregunta…"),
            );
        });
        ui.horizontal(|ui| {
            if is_streaming {
                if ui.button("Detener").clicked() {
                    actions.push(CentralAction::StopGeneration);
                }
            } else if ui.button("Enviar").clicked() {
                actions.push(CentralAction::SendMessage);
            }
            ui.label(format!(
                "Temperatura {:.1} · máx {} tokens",
                self.config.model.temperature, self.config.model.max_tokens
            ));
        });
        self.chat_input = input;

        if let Some(error) = &self.chat_error {
            ui.colored_label(egui::Color32::RED, error);

            let fallback = session
                .fallback_provider
                .as_deref()
                .and_then(ProviderId::from_key);
            let offer_fallback =
                self.last_failure.as_ref().is_some_and(|failure| {
                    failure.session_id == session.id && failure.availability
                }) && fallback.is_some();

            if offer_fallback {
                if let Some(fallback) = fallback {
                    if ui
                        .button(format!("Usar {} y reintentar", fallback.display_name()))
                        .clicked()
                    {
                        actions.push(CentralAction::RetryWithFallback);
                    }
                }
            }
        }

        actions
    }

    /// ¿El proveedor tiene credenciales listas para usar? (según la caché de claves)
    fn provider_ready(&self, provider: ProviderId) -> bool {
        matches!(
            self.key_status.get(&provider),
            Some(KeyStatus::Stored { .. } | KeyStatus::Environment { .. } | KeyStatus::Ok { .. })
        )
    }

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

    fn keys_window(&mut self, ctx: &egui::Context) {
        if !self.keys_open {
            return;
        }

        let mut open = true;
        let mut actions: Vec<KeyAction> = Vec::new();
        let statuses = self.key_status.clone();
        let keyring_available = self.keyring_available;

        egui::Window::new("Claves de API")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(660.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                if !keyring_available {
                    ui.colored_label(
                        egui::Color32::from_rgb(200, 140, 0),
                        "El llavero del sistema no está disponible; se usarán las variables de entorno.",
                    );
                }
                ui.small(
                    "Las claves se guardan cifradas en el almacén del sistema operativo \
                     y nunca se muestran completas.",
                );
                ui.add_space(8.0);

                egui::Grid::new("keys_grid")
                    .num_columns(3)
                    .spacing([10.0, 10.0])
                    .show(ui, |ui| {
                        for provider in ProviderId::ALL {
                            ui.vertical(|ui| {
                                ui.strong(provider.display_name());
                                ui.small(key_status_text(statuses.get(&provider)));
                            });

                            let input = self.key_inputs.entry(provider).or_default();
                            ui.add(
                                egui::TextEdit::singleline(input)
                                    .password(true)
                                    .desired_width(220.0)
                                    .hint_text("pega la clave"),
                            );

                            ui.horizontal(|ui| {
                                if ui.button("Guardar").clicked() {
                                    actions.push(KeyAction::Save(provider));
                                }
                                if ui.button("Probar").clicked() {
                                    actions.push(KeyAction::Test(provider));
                                }
                                if ui.button("Eliminar").clicked() {
                                    actions.push(KeyAction::Delete(provider));
                                }
                            });
                            ui.end_row();
                        }
                    });

                ui.add_space(8.0);
                let env_vars: Vec<&str> = ProviderId::ALL
                    .iter()
                    .map(|provider| provider.api_key_env())
                    .collect();
                ui.small(format!(
                    "Alternativa sin llavero: define {} antes de abrir la aplicación.",
                    env_vars.join(", ")
                ));
            });

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
        if actions.keys {
            self.refresh_keys();
            self.keys_open = true;
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
                CentralAction::OpenKeys => {
                    self.refresh_keys();
                    self.keys_open = true;
                }
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
}

impl eframe::App for RaggyApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        self.drain_events();
        self.drain_chat_events();
        self.drain_key_tests();
        self.poll_watcher();
        if self.worker.is_some() || self.chat_worker.is_some() || self.watcher.is_some() {
            ctx.request_repaint_after(Duration::from_millis(120));
        }

        self.handle_actions(ui);
        self.delete_dialog(&ctx);
        self.session_dialog_window(&ctx);
        self.settings_window(&ctx);
        self.keys_window(&ctx);
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
