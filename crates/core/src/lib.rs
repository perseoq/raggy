//! `localmind-core` — núcleo compartido de LocalMind RAG Desktop.
//!
//! Contiene los tipos transversales a todos los crates: configuración de la
//! aplicación, localización de directorios de datos y el modelo de errores.
//! No depende de la interfaz gráfica ni de ningún proveedor de LLM.

pub mod config;
pub mod error;
pub mod paths;

pub use config::{AppConfig, ModelConfig, RagConfig, Theme, DEFAULT_TEMPERATURE};
pub use error::{CoreError, Result};
pub use paths::AppPaths;

/// Nombre comercial del producto.
pub const APP_NAME: &str = "LocalMind";

/// Identificador corto usado en rutas y nombres de archivo.
pub const APP_SLUG: &str = "localmind";

/// Versión del crate tomada de `Cargo.toml`.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
