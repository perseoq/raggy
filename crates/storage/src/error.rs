//! Errores de la capa de persistencia.

use std::path::PathBuf;

/// Error de la capa de almacenamiento.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Error devuelto por SQLite.
    #[error("error de base de datos: {0}")]
    Database(#[from] rusqlite::Error),

    /// Error de entrada/salida sobre una ruta concreta.
    #[error("error de E/S en `{path}`: {source}")]
    Io {
        /// Ruta afectada.
        path: PathBuf,
        /// Causa original.
        #[source]
        source: std::io::Error,
    },

    /// La base de datos fue creada por una versión más reciente del software.
    #[error("el esquema está en la versión {found}, superior a la soportada ({supported})")]
    SchemaTooNew {
        /// Versión encontrada en la base de datos.
        found: i64,
        /// Versión máxima soportada por este binario.
        supported: i64,
    },
}

/// Alias de resultado para la capa de almacenamiento.
pub type Result<T> = std::result::Result<T, StorageError>;
