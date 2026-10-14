//! `raggy-core` — núcleo compartido de Raggy Desktop.
//!
//! Contiene los tipos transversales a todos los crates: configuración de la
//! aplicación, localización de directorios de datos y el modelo de errores.
//! No depende de la interfaz gráfica ni de ningún proveedor de LLM.

pub mod artifacts;
pub mod backup;
pub mod chat;
pub mod config;
pub mod error;
pub mod export;
pub mod index;
pub mod paths;
pub mod search;
pub mod secrets;
pub mod watch;

pub use artifacts::{
    artifacts_for_prompt, free_path, relative_to, sanitize_relative, save_artifact, ConflictPolicy,
    SaveOutcome,
};
pub use backup::{export_backup, import_backup, BackupSummary};
pub use chat::{
    compose_request, context_from_retrieved, ChatCommand, ChatEvent, ChatWorker, ProviderFactory,
};
pub use config::{AppConfig, ModelConfig, RagConfig, Theme, DEFAULT_TEMPERATURE};
pub use error::{CoreError, Result};
pub use export::{session_markdown, write_session_export};
pub use index::{
    index_folder, scan_markdown_files, IndexCommand, IndexEvent, IndexOptions, IndexPhase,
    IndexProgress, IndexSummary, IndexWorker,
};
pub use paths::{
    home_dir, migrate_data, resolve, resolve_from, AppPaths, HOME_DIR_NAME, HOME_ENV, SETTINGS_FILE,
};
pub use search::{build_retriever, embedding_coverage, search_folder, EmbeddingCoverage};
pub use secrets::{
    mask, resolve_key, MemoryStore, Secret, SecretError, SecretStore, SettingsStore,
};
pub use watch::FolderWatcher;

/// Nombre comercial del producto.
pub const APP_NAME: &str = "Raggy Desktop";

/// Identificador corto usado en rutas y nombres de archivo.
pub const APP_SLUG: &str = "raggy";

/// Versión del crate tomada de `Cargo.toml`.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
