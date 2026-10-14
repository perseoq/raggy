//! Errores del núcleo.

use std::path::PathBuf;

/// Error transversal del núcleo de la aplicación.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// El sistema operativo no expone un directorio de datos de usuario.
    #[error("no se pudo determinar el directorio de datos del sistema operativo")]
    DataDirUnavailable,

    /// Fallo de entrada/salida sobre una ruta concreta.
    #[error("error de E/S en `{path}`: {source}")]
    Io {
        /// Ruta afectada.
        path: PathBuf,
        /// Causa original.
        #[source]
        source: std::io::Error,
    },

    /// La configuración no es válida a nivel lógico.
    #[error("configuración inválida: {0}")]
    InvalidConfig(String),

    /// Error de indexación con contexto de la ruta afectada.
    #[error("error al indexar `{path}`: {message}")]
    Index {
        /// Ruta de la carpeta o archivo.
        path: PathBuf,
        /// Detalle del problema.
        message: String,
    },

    /// Error procedente de la capa de persistencia.
    #[error(transparent)]
    Storage(#[from] raggy_storage::StorageError),

    /// Error al recorrer el sistema de archivos.
    #[error("error al recorrer archivos: {0}")]
    Walk(#[from] walkdir::Error),

    /// Error al vigilar cambios en el sistema de archivos.
    #[error("error al vigilar la carpeta: {0}")]
    Notify(#[from] notify::Error),

    /// Fallo al serializar la configuración a TOML.
    #[error("no se pudo serializar la configuración: {0}")]
    Serialize(#[from] toml::ser::Error),

    /// Fallo al deserializar la configuración desde TOML.
    #[error("no se pudo interpretar la configuración: {0}")]
    Deserialize(#[from] toml::de::Error),
}

/// Alias de resultado para el núcleo.
pub type Result<T> = std::result::Result<T, CoreError>;
