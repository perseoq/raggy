//! Vigilancia de carpetas para la re-indexación automática (HU-03).
//!
//! Envoltorio sobre `notify` que mantiene el conjunto de carpetas vigiladas y
//! acumula los cambios detectados para que la interfaz los consuma cuando
//! pueda, sin bloquearse.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::Result;

/// Vigilante de cambios en disco sobre una o varias carpetas.
pub struct FolderWatcher {
    watcher: RecommendedWatcher,
    receiver: Receiver<notify::Result<notify::Event>>,
    watched: HashSet<PathBuf>,
}

impl FolderWatcher {
    /// Crea el vigilante nativo del sistema operativo.
    pub fn new() -> Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let watcher = notify::recommended_watcher(move |result| {
            let _ = sender.send(result);
        })?;
        Ok(Self {
            watcher,
            receiver,
            watched: HashSet::new(),
        })
    }

    /// Añade una carpeta a la vigilancia recursiva.
    pub fn watch(&mut self, path: &Path) -> Result<()> {
        if !path.is_dir() || !self.watched.insert(path.to_path_buf()) {
            return Ok(());
        }
        if let Err(error) = self.watcher.watch(path, RecursiveMode::Recursive) {
            self.watched.remove(path);
            return Err(error.into());
        }
        Ok(())
    }

    /// Deja de vigilar una carpeta.
    pub fn unwatch(&mut self, path: &Path) -> Result<()> {
        if self.watched.remove(path) {
            self.watcher.unwatch(path)?;
        }
        Ok(())
    }

    /// Sincroniza el conjunto vigilado con la lista de carpetas dada.
    pub fn sync(&mut self, paths: &[PathBuf]) -> Result<()> {
        let desired: HashSet<PathBuf> = paths.iter().cloned().collect();

        let stale: Vec<PathBuf> = self.watched.difference(&desired).cloned().collect();
        for path in stale {
            self.unwatch(&path)?;
        }

        let missing: Vec<PathBuf> = desired.difference(&self.watched).cloned().collect();
        for path in missing {
            self.watch(&path)?;
        }

        Ok(())
    }

    /// Extrae y deduplica los cambios acumulados desde la última llamada.
    pub fn poll(&self) -> Vec<PathBuf> {
        let mut changed = Vec::new();
        let mut seen = HashSet::new();
        for event in self.receiver.try_iter().flatten() {
            for path in event.paths {
                if seen.insert(path.clone()) {
                    changed.push(path);
                }
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detecta_archivos_nuevos_y_borrados() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("localmind-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("crea directorio");

        let mut watcher = FolderWatcher::new().expect("crea vigilante");
        watcher.watch(&dir).expect("vigila");

        std::fs::write(dir.join("nuevo.md"), "# Nuevo\n").expect("escribe");
        std::fs::remove_file(dir.join("nuevo.md")).expect("borra");

        // Los eventos de inotify pueden tardar; se espera de forma acotada.
        let mut paths = Vec::new();
        for _ in 0..100 {
            paths.extend(watcher.poll());
            if paths.iter().any(|p| p.ends_with("nuevo.md")) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        assert!(
            paths.iter().any(|path| path.ends_with("nuevo.md")),
            "no se detectó el cambio: {paths:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
