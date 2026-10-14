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

    /// Fallo al serializar la configuración a TOML.
    #[error("no se pudo serializar la configuración: {0}")]
    Serialize(#[from] toml::ser::Error),

    /// Fallo al deserializar la configuración desde TOML.
    #[error("no se pudo interpretar la configuración: {0}")]
    Deserialize(#[from] toml::de::Error),
}

/// Alias de resultado para el núcleo.
pub type Result<T> = std::result::Result<T, CoreError>;
