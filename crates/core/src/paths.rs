//! Localización de los directorios de datos de la aplicación.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::error::{CoreError, Result};
use crate::APP_SLUG;

/// Variable de entorno que permite sobrescribir el directorio de datos.
///
/// Útil para desarrollo, pruebas y para forzar una ubicación concreta.
pub const DATA_DIR_ENV: &str = "RAGGY_DATA_DIR";

/// Archivo que recuerda la ubicación de datos elegida por el usuario (HU-28).
pub const LOCATION_FILE: &str = "location.toml";

/// Contenido del archivo de ubicación.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocationConfig {
    /// Directorio de datos elegido; `None` usa el del sistema.
    pub data_dir: Option<PathBuf>,
}

/// Rutas canónicas usadas por la aplicación dentro del directorio de datos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    /// Directorio raíz de datos.
    pub data_dir: PathBuf,
}

impl AppPaths {
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

/// Directorio de datos por defecto del sistema operativo.
pub fn default_data_dir() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("dev", "perseoq", APP_SLUG)
        .ok_or(CoreError::DataDirUnavailable)?;
    Ok(dirs.data_dir().to_path_buf())
}

/// Ruta del archivo de ubicación dentro del directorio por defecto.
pub fn location_file(base: &Path) -> PathBuf {
    base.join(LOCATION_FILE)
}

/// Resuelve las rutas de trabajo según la configuración guardada.
///
/// Prioridad: variable de entorno, archivo de ubicación y, por último, el
/// directorio estándar del sistema.
pub fn resolve() -> Result<AppPaths> {
    if let Some(dir) = std::env::var_os(DATA_DIR_ENV) {
        if !dir.is_empty() {
            return Ok(AppPaths::with_data_dir(dir));
        }
    }

    let base = default_data_dir()?;
    Ok(resolve_from(&base))
}

/// Resuelve las rutas a partir de un directorio base (sin variables de entorno).
pub fn resolve_from(base: &Path) -> AppPaths {
    if let Ok(raw) = std::fs::read_to_string(location_file(base)) {
        if let Ok(location) = toml::from_str::<LocationConfig>(&raw) {
            if let Some(dir) = location.data_dir {
                return AppPaths::with_data_dir(dir);
            }
        }
    }
    AppPaths::with_data_dir(base)
}

/// Guarda la ubicación de datos elegida por el usuario (HU-28).
pub fn set_location(data_dir: &Path) -> Result<()> {
    let base = default_data_dir()?;
    let config = LocationConfig {
        data_dir: Some(data_dir.to_path_buf()),
    };
    let serialized = toml::to_string_pretty(&config)?;
    std::fs::create_dir_all(&base).map_err(|source| CoreError::Io {
        path: base.clone(),
        source,
    })?;
    let path = location_file(&base);
    std::fs::write(&path, serialized).map_err(|source| CoreError::Io { path, source })?;
    Ok(())
}

/// Migra los datos existentes a otro directorio (HU-28).
///
/// La base de datos se copia con una instantánea consistente; el resto de
/// archivos (configuración, índices, artefactos y logs) se copian tal cual.
pub fn migrate_data(
    from: &AppPaths,
    to: &AppPaths,
    storage: &raggy_storage::Storage,
) -> Result<usize> {
    if from.data_dir == to.data_dir {
        return Ok(0);
    }
    to.ensure_dirs()?;

    storage.backup_to(&to.db_path())?;
    let mut copied = 1usize;

    let config = from.config_path();
    if config.is_file() {
        std::fs::copy(&config, to.config_path()).map_err(|source| CoreError::Io {
            path: to.config_path(),
            source,
        })?;
        copied += 1;
    }

    for (source, destination) in [
        (from.indexes_dir(), to.indexes_dir()),
        (from.artifacts_dir(), to.artifacts_dir()),
        (from.logs_dir(), to.logs_dir()),
    ] {
        copy_tree(&source, &destination, &mut copied)?;
    }

    Ok(copied)
}

fn copy_tree(from: &Path, to: &Path, copied: &mut usize) -> Result<()> {
    if !from.is_dir() {
        return Ok(());
    }

    for entry in WalkDir::new(from) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(from).unwrap_or(entry.path());
        let destination = to.join(relative);

        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&destination).map_err(|source| CoreError::Io {
                path: destination,
                source,
            })?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent).map_err(|source| CoreError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            std::fs::copy(entry.path(), &destination).map_err(|source| CoreError::Io {
                path: destination,
                source,
            })?;
            *copied += 1;
        }
    }

    Ok(())
}

impl AsRef<OsStr> for AppPaths {
    fn as_ref(&self) -> &OsStr {
        self.data_dir.as_os_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("raggy-paths-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("crea directorio temporal");
        path
    }

    #[test]
    fn rutas_derivadas_cuelgan_del_directorio_de_datos() {
        let paths = AppPaths::with_data_dir("/tmp/raggy-test");
        assert_eq!(
            paths.config_path(),
            PathBuf::from("/tmp/raggy-test/config.toml")
        );
        assert_eq!(
            paths.db_path(),
            PathBuf::from("/tmp/raggy-test/raggy.sqlite3")
        );
        assert_eq!(
            paths.indexes_dir(),
            PathBuf::from("/tmp/raggy-test/indexes")
        );
    }

    #[test]
    fn ensure_dirs_crea_el_arbol() {
        let dir = temp_dir("dirs");
        let paths = AppPaths::with_data_dir(&dir);
        paths.ensure_dirs().expect("debería crear el árbol");
        assert!(paths.data_dir.is_dir());
        assert!(paths.indexes_dir().is_dir());
        assert!(paths.artifacts_dir().is_dir());
        assert!(paths.logs_dir().is_dir());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_from_usa_el_archivo_de_ubicacion() {
        let base = temp_dir("base");
        let custom = temp_dir("custom");

        // Sin archivo de ubicación se usa el directorio base.
        assert_eq!(resolve_from(&base).data_dir, base);

        // Con archivo de ubicación se usa el directorio indicado.
        let config = LocationConfig {
            data_dir: Some(custom.clone()),
        };
        std::fs::write(
            location_file(&base),
            toml::to_string_pretty(&config).expect("serializa"),
        )
        .expect("escribe ubicación");
        assert_eq!(resolve_from(&base).data_dir, custom);

        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&custom);
    }
}
