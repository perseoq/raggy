//! Rutas de la aplicación y resolución del directorio de datos.
//!
//! La carpeta base es `~/.raggydesktop` (configurable con `RAGGY_HOME`). Dentro
//! vive `settings.json`, que contiene la configuración **y las claves de API**, y
//! por defecto también la base de datos, los índices, los artefactos y los
//! registros. La propia configuración puede apuntar a otro directorio de datos.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::error::{CoreError, Result};
use crate::APP_SLUG;

/// Variable de entorno que cambia la carpeta base de la aplicación.
pub const HOME_ENV: &str = "RAGGY_HOME";

/// Nombre de la carpeta base dentro del directorio personal.
pub const HOME_DIR_NAME: &str = ".raggydesktop";

/// Archivo de configuración y credenciales.
pub const SETTINGS_FILE: &str = "settings.json";

/// Rutas canónicas usadas por la aplicación.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    /// Carpeta base (`~/.raggydesktop`), donde vive `settings.json`.
    pub home_dir: PathBuf,
    /// Directorio de datos (base de datos, índices, artefactos y registros).
    pub data_dir: PathBuf,
}

impl AppPaths {
    /// Construye las rutas con un directorio de datos igual a la carpeta base.
    pub fn with_home(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        Self {
            data_dir: home.clone(),
            home_dir: home,
        }
    }

    /// Construye las rutas con una carpeta base y un directorio de datos.
    pub fn with_data_dir(home: impl Into<PathBuf>, data_dir: impl Into<PathBuf>) -> Self {
        Self {
            home_dir: home.into(),
            data_dir: data_dir.into(),
        }
    }

    /// Archivo de configuración y credenciales (`settings.json`).
    pub fn settings_path(&self) -> PathBuf {
        self.home_dir.join(SETTINGS_FILE)
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

    /// Directorio de subidas de archivos de código.
    pub fn uploads_dir(&self) -> PathBuf {
        self.data_dir.join("uploads")
    }

    /// Directorio de registros (logs).
    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    /// Crea el árbol de directorios si no existe.
    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [
            self.home_dir.clone(),
            self.data_dir.clone(),
            self.indexes_dir(),
            self.artifacts_dir(),
            self.uploads_dir(),
            self.logs_dir(),
        ] {
            std::fs::create_dir_all(&dir).map_err(|source| CoreError::Io { path: dir, source })?;
        }
        Ok(())
    }
}

/// Resuelve las rutas de trabajo según el entorno y la configuración guardada.
///
/// La carpeta base sale de `RAGGY_HOME` o de `~/.raggydesktop`; el directorio de
/// datos, del campo `data_dir` de `settings.json` o de la propia carpeta base.
pub fn resolve() -> Result<AppPaths> {
    Ok(resolve_from(&home_dir()?))
}

/// Carpeta base de la aplicación.
pub fn home_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os(HOME_ENV) {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }

    let base = directories::BaseDirs::new().ok_or(CoreError::DataDirUnavailable)?;
    Ok(base.home_dir().join(HOME_DIR_NAME))
}

/// Resuelve las rutas a partir de una carpeta base explícita.
pub fn resolve_from(home: &Path) -> AppPaths {
    let data_dir =
        data_dir_from_settings(&home.join(SETTINGS_FILE)).unwrap_or_else(|| home.to_path_buf());
    AppPaths::with_data_dir(home, data_dir)
}

/// Lee el campo `data_dir` de `settings.json`, si está definido.
fn data_dir_from_settings(path: &Path) -> Option<PathBuf> {
    let raw = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    value
        .get("data_dir")
        .and_then(|dir| dir.as_str())
        .filter(|dir| !dir.trim().is_empty())
        .map(PathBuf::from)
}

/// Migra los datos existentes a otro directorio (HU-28).
///
/// La base de datos se copia con una instantánea consistente; el resto de
/// archivos (índices, artefactos y registros) se copian tal cual. El archivo
/// `settings.json` no se copia aquí: lo gestiona la configuración.
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

    for (source, destination) in [
        (from.indexes_dir(), to.indexes_dir()),
        (from.artifacts_dir(), to.artifacts_dir()),
        (from.uploads_dir(), to.uploads_dir()),
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
        let paths = AppPaths::with_home("/tmp/raggy-home");
        assert_eq!(
            paths.settings_path(),
            PathBuf::from("/tmp/raggy-home/settings.json")
        );
        assert_eq!(
            paths.db_path(),
            PathBuf::from("/tmp/raggy-home/raggy.sqlite3")
        );
        assert_eq!(
            paths.indexes_dir(),
            PathBuf::from("/tmp/raggy-home/indexes")
        );
    }

    #[test]
    fn ensure_dirs_crea_el_arbol() {
        let dir = temp_dir("dirs");
        let paths = AppPaths::with_home(&dir);
        paths.ensure_dirs().expect("debería crear el árbol");
        assert!(paths.home_dir.is_dir());
        assert!(paths.logs_dir().is_dir());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn el_directorio_de_datos_sale_de_settings_json() {
        let home = temp_dir("home");
        let custom = temp_dir("custom");

        // Sin settings.json, el directorio de datos es la carpeta base.
        assert_eq!(resolve_from(&home).data_dir, home);

        // Con `data_dir` en settings.json, se usa ese directorio.
        std::fs::write(
            home.join(SETTINGS_FILE),
            format!("{{\"data_dir\": \"{}\"}}", custom.display()),
        )
        .expect("escribe settings.json");
        assert_eq!(resolve_from(&home).data_dir, custom);

        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&custom);
    }
}
