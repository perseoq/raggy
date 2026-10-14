//! Prueba de integración del pipeline completo de indexación (HU-01 a HU-04):
//! sistema de archivos real + SQLite en disco + hilo indexador.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use raggy_core::{IndexEvent, IndexWorker};
use raggy_rag::ChunkingOptions;
use raggy_storage::{Folder, FolderStatus, Storage};

struct Temporary {
    path: PathBuf,
}

impl Temporary {
    fn new(tag: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("raggy-it-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("crea directorio temporal");
        Self { path }
    }

    fn join(&self, relative: &str) -> PathBuf {
        self.path.join(relative)
    }

    fn write(&self, relative: &str, content: &str) {
        let full = self.join(relative);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("crea subdirectorio");
        }
        std::fs::write(full, content).expect("escribe archivo");
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn options() -> ChunkingOptions {
    ChunkingOptions {
        max_chars: 300,
        overlap_chars: 40,
    }
}

fn wait_for_finish(worker: &IndexWorker, timeout: Duration) -> Option<raggy_core::IndexSummary> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        for event in worker.drain() {
            if let IndexEvent::Finished(summary) = event {
                return Some(summary);
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    None
}

#[test]
fn el_worker_indexa_y_actualiza_de_forma_incremental() {
    let docs = Temporary::new("docs");
    docs.write(
        "guia.md",
        "# Guía\n\nIntroducción.\n\n## Uso\n\nEjecuta el binario.\n\n```bash\nraggy --help\n```\n",
    );
    docs.write("notas/ideas.md", "# Ideas\n\n- una\n- dos\n");

    let data = Temporary::new("data");
    let db_path = data.join("raggy.sqlite3");

    let folder = {
        let storage = Storage::open(&db_path).expect("abre la base de datos");
        let folder = Folder::new(docs.path.to_string_lossy().to_string(), "docs");
        storage.upsert_folder(&folder).expect("registra la carpeta");
        folder
    };

    let worker = IndexWorker::spawn(db_path.clone()).expect("arranca el indexador");
    worker.index_folder(folder.id.clone(), docs.path.clone(), options());

    let summary = wait_for_finish(&worker, Duration::from_secs(10)).expect("termina la indexación");
    assert_eq!(summary.files_total, 2);
    assert_eq!(summary.files_indexed, 2);
    assert!(summary.chunks_written >= 2);

    // La reindexación sin cambios no vuelve a procesar archivos.
    worker.index_folder(folder.id.clone(), docs.path.clone(), options());
    let summary =
        wait_for_finish(&worker, Duration::from_secs(10)).expect("termina la reindexación");
    assert_eq!(summary.files_indexed, 0);
    assert_eq!(summary.files_skipped, 2);

    // Un archivo nuevo se incorpora.
    docs.write("tercero.md", "# Tercero\n\nnuevo contenido\n");
    worker.index_folder(folder.id.clone(), docs.path.clone(), options());
    let summary = wait_for_finish(&worker, Duration::from_secs(10)).expect("termina la tercera");
    assert_eq!(summary.files_indexed, 1);
    assert_eq!(summary.files_skipped, 2);

    drop(worker);

    let storage = Storage::open(&db_path).expect("reabre la base de datos");
    let documents = storage
        .list_documents(&folder.id)
        .expect("lista documentos");
    assert_eq!(documents.len(), 3);
    assert!(documents.iter().all(|document| document.chunk_count >= 1));

    let stored = storage
        .get_folder(&folder.id)
        .expect("lee")
        .expect("existe");
    assert_eq!(stored.status, FolderStatus::Indexed);
    assert_eq!(stored.file_count, 3);

    let chunks = storage
        .list_chunks_for_path(&folder.id, "guia.md")
        .expect("lista chunks");
    assert!(chunks.iter().any(|chunk| chunk.metadata.heading.is_some()));
    assert!(chunk_keeps_code_block(&chunks));
}

/// Comprueba que ningún fragmento contiene un cerco de código impar.
fn chunk_keeps_code_block(chunks: &[raggy_rag::Chunk]) -> bool {
    chunks.iter().all(|chunk| {
        let fences = chunk
            .content
            .lines()
            .filter(|line| line.trim_start().starts_with("```"))
            .count();
        fences % 2 == 0
    })
}
