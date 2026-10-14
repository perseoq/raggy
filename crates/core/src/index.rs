//! Indexación de carpetas Markdown (HU-01 a HU-04).
//!
//! El motor recorre una carpeta, divide cada `.md` en fragmentos con el
//! [`MarkdownChunker`] y los persiste. La re-indexación es incremental: solo se
//! reprocesan los archivos cuyo hash de contenido cambió, y los que ya no
//! existen se retiran del índice.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use walkdir::WalkDir;

use localmind_rag::{ChunkSource, Chunker, MarkdownChunker};
use localmind_storage::{Document, Folder, FolderStatus, Storage};

use crate::error::{CoreError, Result};

/// Fase del proceso de indexación, para informar a la interfaz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexPhase {
    /// Recorriendo el sistema de archivos.
    Scanning,
    /// Procesando archivos.
    Indexing,
    /// Finalizado.
    Done,
    /// Terminado con error.
    Failed,
}

/// Progreso de indexación de una carpeta.
#[derive(Debug, Clone)]
pub struct IndexProgress {
    /// Carpeta en proceso.
    pub folder_id: String,
    /// Fase actual.
    pub phase: IndexPhase,
    /// Archivos procesados hasta ahora.
    pub done: usize,
    /// Total de archivos detectados.
    pub total: usize,
}

/// Resumen del resultado de una indexación.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexSummary {
    /// Carpeta procesada.
    pub folder_id: String,
    /// Archivos `.md` encontrados.
    pub files_total: usize,
    /// Archivos nuevos o modificados indexados.
    pub files_indexed: usize,
    /// Archivos sin cambios (omitidos).
    pub files_skipped: usize,
    /// Archivos retirados del índice por haber desaparecido.
    pub files_removed: usize,
    /// Archivos ilegibles que se reportaron y se omitieron.
    pub files_failed: usize,
    /// Fragmentos escritos en el índice.
    pub chunks_written: usize,
}

/// Recorre recursivamente `root` y devuelve los archivos `.md`.
pub fn scan_markdown_files(root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| is_markdown(path))
        .collect()
}

/// ¿La ruta tiene extensión Markdown?
pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

/// Indexa una carpeta de forma incremental.
///
/// `report` recibe el progreso para poder mostrar una barra en la interfaz.
pub fn index_folder(
    storage: &mut Storage,
    folder_id: &str,
    root: &Path,
    options: &localmind_rag::ChunkingOptions,
    report: &mut dyn FnMut(IndexProgress),
) -> Result<IndexSummary> {
    if !root.is_dir() {
        return Err(CoreError::Index {
            path: root.to_path_buf(),
            message: "la carpeta no existe o no es accesible".to_string(),
        });
    }

    storage.set_folder_status(folder_id, FolderStatus::Indexing, 0)?;

    let mut files = scan_markdown_files(root);
    files.sort();
    let total = files.len();

    report(IndexProgress {
        folder_id: folder_id.to_string(),
        phase: IndexPhase::Scanning,
        done: 0,
        total,
    });

    let existing: HashMap<String, Document> = storage
        .list_documents(folder_id)?
        .into_iter()
        .map(|document| (document.relative_path.clone(), document))
        .collect();

    let chunker = MarkdownChunker::new();
    let mut summary = IndexSummary {
        folder_id: folder_id.to_string(),
        files_total: total,
        ..IndexSummary::default()
    };
    let mut seen: HashSet<String> = HashSet::new();

    for (position, path) in files.iter().enumerate() {
        let relative = relative_path(root, path);
        seen.insert(relative.clone());

        report(IndexProgress {
            folder_id: folder_id.to_string(),
            phase: IndexPhase::Indexing,
            done: position,
            total,
        });

        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) => {
                tracing::warn!(path = %path.display(), "archivo ilegible, se omite: {error}");
                summary.files_failed += 1;
                continue;
            }
        };

        let metadata = std::fs::metadata(path).ok();
        let size_bytes = metadata.as_ref().map_or(0, |m| m.len() as i64);
        let modified_at = metadata.as_ref().map_or(0, modified_millis);
        let content_hash = blake3::hash(content.as_bytes()).to_hex().to_string();

        if existing
            .get(&relative)
            .is_some_and(|document| document.content_hash == content_hash)
        {
            summary.files_skipped += 1;
            continue;
        }

        let source = ChunkSource {
            relative_path: &relative,
            content: &content,
        };
        let chunks = chunker.chunk(&source, options);

        storage.replace_chunks(folder_id, &relative, &chunks)?;
        let document = Document::new(
            folder_id,
            relative,
            content_hash,
            size_bytes,
            modified_at,
            chunks.len() as i64,
        );
        storage.upsert_document(&document)?;

        summary.files_indexed += 1;
        summary.chunks_written += chunks.len();
    }

    for relative in existing.keys() {
        if !seen.contains(relative) {
            storage.delete_chunks_for_path(folder_id, relative)?;
            storage.delete_document(folder_id, relative)?;
            summary.files_removed += 1;
        }
    }

    storage.set_folder_status(folder_id, FolderStatus::Indexed, total as i64)?;

    report(IndexProgress {
        folder_id: folder_id.to_string(),
        phase: IndexPhase::Done,
        done: total,
        total,
    });

    Ok(summary)
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn modified_millis(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// Comandos aceptados por el hilo indexador.
#[derive(Debug, Clone)]
pub enum IndexCommand {
    /// Indexa (de forma incremental) una carpeta concreta.
    IndexFolder {
        /// Identificador de la carpeta.
        folder_id: String,
        /// Raíz en disco.
        root: PathBuf,
        /// Parámetros de chunking vigentes.
        options: localmind_rag::ChunkingOptions,
    },
    /// Reindexa todas las carpetas registradas.
    ReindexAll {
        /// Parámetros de chunking vigentes.
        options: localmind_rag::ChunkingOptions,
    },
    /// Detiene el hilo.
    Shutdown,
}

/// Eventos emitidos por el hilo indexador.
#[derive(Debug, Clone)]
pub enum IndexEvent {
    /// Progreso de una carpeta.
    Progress(IndexProgress),
    /// Indexación completada con éxito.
    Finished(IndexSummary),
    /// Indexación fallida.
    Failed {
        /// Carpeta afectada.
        folder_id: String,
        /// Descripción del error.
        message: String,
    },
}

/// Hilo dedicado que ejecuta las indexaciones fuera del hilo de la interfaz.
pub struct IndexWorker {
    command_tx: Sender<IndexCommand>,
    event_rx: Receiver<IndexEvent>,
    handle: Option<JoinHandle<()>>,
}

impl IndexWorker {
    /// Arranca el hilo indexador con su propia conexión a la base de datos.
    pub fn spawn(db_path: PathBuf) -> Result<Self> {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();

        let handle = thread::Builder::new()
            .name("localmind-indexer".to_string())
            .spawn(move || worker_loop(&db_path, command_rx, event_tx))
            .map_err(|source| CoreError::Io {
                path: PathBuf::from("<hilo indexador>"),
                source,
            })?;

        Ok(Self {
            command_tx,
            event_rx,
            handle: Some(handle),
        })
    }

    /// Encola la indexación de una carpeta.
    pub fn index_folder(
        &self,
        folder_id: impl Into<String>,
        root: PathBuf,
        options: localmind_rag::ChunkingOptions,
    ) {
        let _ = self.command_tx.send(IndexCommand::IndexFolder {
            folder_id: folder_id.into(),
            root,
            options,
        });
    }

    /// Encola la reindexación de todas las carpetas.
    pub fn reindex_all(&self, options: localmind_rag::ChunkingOptions) {
        let _ = self.command_tx.send(IndexCommand::ReindexAll { options });
    }

    /// Extrae todos los eventos disponibles sin bloquear.
    pub fn drain(&self) -> Vec<IndexEvent> {
        self.event_rx.try_iter().collect()
    }
}

impl Drop for IndexWorker {
    fn drop(&mut self) {
        let _ = self.command_tx.send(IndexCommand::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn worker_loop(db_path: &Path, command_rx: Receiver<IndexCommand>, event_tx: Sender<IndexEvent>) {
    let mut storage = match Storage::open(db_path) {
        Ok(storage) => storage,
        Err(error) => {
            tracing::error!("el hilo indexador no pudo abrir la base de datos: {error}");
            return;
        }
    };

    while let Ok(command) = command_rx.recv() {
        match command {
            IndexCommand::Shutdown => break,
            IndexCommand::IndexFolder {
                folder_id,
                root,
                options,
            } => {
                run_index(&mut storage, &event_tx, &folder_id, &root, &options);
            }
            IndexCommand::ReindexAll { options } => {
                let folders: Vec<Folder> = match storage.list_folders() {
                    Ok(folders) => folders,
                    Err(error) => {
                        tracing::error!("no se pudieron listar las carpetas: {error}");
                        continue;
                    }
                };
                for folder in folders {
                    run_index(
                        &mut storage,
                        &event_tx,
                        &folder.id,
                        Path::new(&folder.path),
                        &options,
                    );
                }
            }
        }
    }
}

fn run_index(
    storage: &mut Storage,
    event_tx: &Sender<IndexEvent>,
    folder_id: &str,
    root: &Path,
    options: &localmind_rag::ChunkingOptions,
) {
    let mut report = |progress: IndexProgress| {
        let _ = event_tx.send(IndexEvent::Progress(progress));
    };

    match index_folder(storage, folder_id, root, options, &mut report) {
        Ok(summary) => {
            let _ = event_tx.send(IndexEvent::Finished(summary));
        }
        Err(error) => {
            let _ = storage.set_folder_status(folder_id, FolderStatus::Error, 0);
            tracing::error!(folder_id, "indexación fallida: {error}");
            let _ = event_tx.send(IndexEvent::Failed {
                folder_id: folder_id.to_string(),
                message: error.to_string(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use localmind_rag::ChunkingOptions;

    struct TemporalDir {
        path: PathBuf,
    }

    impl TemporalDir {
        fn new(tag: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!("localmind-index-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("crea directorio temporal");
            Self { path }
        }

        fn write(&self, relative: &str, content: &str) {
            let full = self.path.join(relative);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("crea subdirectorio");
            }
            std::fs::write(full, content).expect("escribe archivo");
        }

        fn remove(&self, relative: &str) {
            std::fs::remove_file(self.path.join(relative)).expect("borra archivo");
        }
    }

    impl Drop for TemporalDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn options() -> ChunkingOptions {
        ChunkingOptions {
            max_chars: 200,
            overlap_chars: 20,
        }
    }

    #[test]
    fn indexa_archivos_md_de_forma_recursiva() {
        let dir = TemporalDir::new("recursivo");
        dir.write("uno.md", "# Uno\n\ntexto uno\n");
        dir.write("sub/dos.md", "# Dos\n\ntexto dos\n");
        dir.write("ignorado.txt", "no es markdown");
        dir.write("sub/tres.markdown", "tampoco");

        let mut storage = Storage::open_in_memory().expect("abre");
        let folder = Folder::new(dir.path.to_string_lossy().to_string(), "docs");
        storage.upsert_folder(&folder).expect("guarda carpeta");

        let mut sin_progreso = |_| {};
        let summary = index_folder(
            &mut storage,
            &folder.id,
            &dir.path,
            &options(),
            &mut sin_progreso,
        )
        .expect("indexa");

        assert_eq!(summary.files_total, 2);
        assert_eq!(summary.files_indexed, 2);
        assert_eq!(storage.list_documents(&folder.id).expect("lista").len(), 2);
        assert!(storage.count_chunks(&folder.id).expect("cuenta") >= 2);
    }

    #[test]
    fn la_reindexacion_omite_archivos_sin_cambios() {
        let dir = TemporalDir::new("incremental");
        dir.write("uno.md", "# Uno\n\ntexto uno\n");

        let mut storage = Storage::open_in_memory().expect("abre");
        let folder = Folder::new(dir.path.to_string_lossy().to_string(), "docs");
        storage.upsert_folder(&folder).expect("guarda");

        let mut sin_progreso = |_| {};
        let first = index_folder(
            &mut storage,
            &folder.id,
            &dir.path,
            &options(),
            &mut sin_progreso,
        )
        .expect("primera");
        assert_eq!(first.files_indexed, 1);

        let second = index_folder(
            &mut storage,
            &folder.id,
            &dir.path,
            &options(),
            &mut sin_progreso,
        )
        .expect("segunda");
        assert_eq!(second.files_indexed, 0);
        assert_eq!(second.files_skipped, 1);

        dir.write("dos.md", "# Dos\n\nnuevo\n");
        let third = index_folder(
            &mut storage,
            &folder.id,
            &dir.path,
            &options(),
            &mut sin_progreso,
        )
        .expect("tercera");
        assert_eq!(third.files_indexed, 1);
        assert_eq!(third.files_skipped, 1);
    }

    #[test]
    fn retira_los_archivos_eliminados() {
        let dir = TemporalDir::new("borrado");
        dir.write("uno.md", "# Uno\n");
        dir.write("dos.md", "# Dos\n");

        let mut storage = Storage::open_in_memory().expect("abre");
        let folder = Folder::new(dir.path.to_string_lossy().to_string(), "docs");
        storage.upsert_folder(&folder).expect("guarda");

        let mut sin_progreso = |_| {};
        index_folder(
            &mut storage,
            &folder.id,
            &dir.path,
            &options(),
            &mut sin_progreso,
        )
        .expect("inicial");

        dir.remove("dos.md");
        let summary = index_folder(
            &mut storage,
            &folder.id,
            &dir.path,
            &options(),
            &mut sin_progreso,
        )
        .expect("tras borrar");

        assert_eq!(summary.files_removed, 1);
        assert_eq!(storage.list_documents(&folder.id).expect("lista").len(), 1);
    }

    #[test]
    fn marca_la_carpeta_como_indexada() {
        let dir = TemporalDir::new("estado");
        dir.write("uno.md", "# Uno\n");

        let mut storage = Storage::open_in_memory().expect("abre");
        let folder = Folder::new(dir.path.to_string_lossy().to_string(), "docs");
        storage.upsert_folder(&folder).expect("guarda");

        let mut sin_progreso = |_| {};
        index_folder(
            &mut storage,
            &folder.id,
            &dir.path,
            &options(),
            &mut sin_progreso,
        )
        .expect("indexa");

        let stored = storage
            .get_folder(&folder.id)
            .expect("lee")
            .expect("existe");
        assert_eq!(stored.status, FolderStatus::Indexed);
        assert_eq!(stored.file_count, 1);
    }

    #[test]
    fn falla_si_la_carpeta_no_existe() {
        let mut storage = Storage::open_in_memory().expect("abre");
        let folder = Folder::new("/ruta/inexistente/xyz", "docs");
        storage.upsert_folder(&folder).expect("guarda");

        let mut sin_progreso = |_| {};
        let result = index_folder(
            &mut storage,
            &folder.id,
            Path::new("/ruta/inexistente/xyz"),
            &options(),
            &mut sin_progreso,
        );
        assert!(matches!(result, Err(CoreError::Index { .. })));
    }
}
