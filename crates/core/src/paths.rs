//! Localización de los directorios de datos de la aplicación.

use std::ffi::OsStr;
use std::path::PathBuf;

use crate::error::{CoreError, Result};
use crate::APP_SLUG;

/// Variable de entorno que permite sobrescribir el directorio de datos.
///
/// Útil para desarrollo, pruebas y para los casos "portables" descritos en
/// HU-28 (directorio de datos configurable).
pub const DATA_DIR_ENV: &str = "LOCALMIND_DATA_DIR";

/// Rutas canónicas usadas por la aplicación dentro del directorio de datos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    /// Directorio raíz de datos.
    pub data_dir: PathBuf,
}

impl AppPaths {
    /// Resuelve el directorio de datos respetando `LOCALMIND_DATA_DIR` y, en su
    /// defecto, el directorio estándar del sistema operativo.
    pub fn resolve() -> Result<Self> {
        if let Some(dir) = std::env::var_os(DATA_DIR_ENV) {
            if !dir.is_empty() {
                return Ok(Self::with_data_dir(dir));
            }
        }

        let dirs = directories::ProjectDirs::from("dev", "perseoq", APP_SLUG)
            .ok_or(CoreError::DataDirUnavailable)?;
        Ok(Self::with_data_dir(dirs.data_dir()))
    }

    /// Construye las rutas sobre un directorio de datos explícito.
    pub fn with_data_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: dir.into(),
        }
    }

    /// Archivo de configuración (`config.toml`).
    pub fn config_path(&self) -> PathBuf {
        self.data_dir.join("config.toml")
    }

    /// Base de datos SQLite principal.
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join(format!("{APP_SLUG}.sqlite3"))
    }

    /// Directorio de índices vectoriales, uno por carpeta indexada.
    pub fn indexes_dir(&self) -> PathBuf {
        self.data_dir.join("indexes")
    }

    /// Directorio donde se registran los artefactos generados.
    pub fn artifacts_dir(&self) -> PathBuf {
        self.data_dir.join("artifacts")
    }

    /// Directorio de registros (logs).
    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    /// Crea el árbol de directorios si no existe.
    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [
            self.data_dir.clone(),
            self.indexes_dir(),
            self.artifacts_dir(),
            self.logs_dir(),
        ] {
            std::fs::create_dir_all(&dir).map_err(|source| CoreError::Io { path: dir, source })?;
        }
        Ok(())
    }
}

impl AsRef<OsStr> for AppPaths {
    fn as_ref(&self) -> &OsStr {
        self.data_dir.as_os_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rutas_derivadas_cuelgan_del_directorio_de_datos() {
        let paths = AppPaths::with_data_dir("/tmp/localmind-test");
        assert_eq!(
            paths.config_path(),
            PathBuf::from("/tmp/localmind-test/config.toml")
        );
        assert_eq!(
            paths.db_path(),
            PathBuf::from("/tmp/localmind-test/localmind.sqlite3")
        );
        assert_eq!(
            paths.indexes_dir(),
            PathBuf::from("/tmp/localmind-test/indexes")
        );
    }

    #[test]
    fn ensure_dirs_crea_el_arbol() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("localmind-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let paths = AppPaths::with_data_dir(&dir);
        paths.ensure_dirs().expect("debería crear el árbol");

        assert!(paths.data_dir.is_dir());
        assert!(paths.indexes_dir().is_dir());
        assert!(paths.artifacts_dir().is_dir());
        assert!(paths.logs_dir().is_dir());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
