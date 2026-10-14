//! `raggy-core` — núcleo compartido de Raggy Desktop.
//!
//! Contiene los tipos transversales a todos los crates: configuración de la
//! aplicación, localización de directorios de datos y el modelo de errores.
//! No depende de la interfaz gráfica ni de ningún proveedor de LLM.

pub mod chat;
pub mod config;
pub mod error;
pub mod index;
pub mod paths;
pub mod search;
pub mod secrets;
pub mod watch;

pub use chat::{
    compose_request, context_from_retrieved, ChatCommand, ChatEvent, ChatWorker, ProviderFactory,
};
pub use config::{AppConfig, ModelConfig, RagConfig, Theme, DEFAULT_TEMPERATURE};
pub use error::{CoreError, Result};
pub use index::{
    index_folder, scan_markdown_files, IndexCommand, IndexEvent, IndexOptions, IndexPhase,
    IndexProgress, IndexSummary, IndexWorker,
};
pub use paths::AppPaths;
pub use search::{build_retriever, embedding_coverage, search_folder, EmbeddingCoverage};
pub use secrets::{mask, resolve_key, KeyringStore, MemoryStore, SecretError, SecretStore};
pub use watch::FolderWatcher;

/// Nombre comercial del producto.
pub const APP_NAME: &str = "Raggy Desktop";

/// Identificador corto usado en rutas y nombres de archivo.
pub const APP_SLUG: &str = "raggy";

/// Versión del crate tomada de `Cargo.toml`.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
