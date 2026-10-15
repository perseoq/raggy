//! Indexación de carpetas y documentos (HU-01 a HU-04).
//!
//! El motor recorre una carpeta, extrae documentos y código, los divide en
//! fragmentos y los persiste. La re-indexación es incremental por contenido y
//! por huella del pipeline (extractores, opciones y herramientas disponibles);
//! los archivos que ya no existen se retiran solo si el recorrido fue completo.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use walkdir::WalkDir;

use raggy_docs::tools::{find_libreoffice, Tools};
use raggy_docs::ExtractOptions;
use raggy_rag::{
    chunk_sections, Chunk, ChunkSource, Chunker, ChunkingOptions, DocumentSection,
    EmbeddingProvider, HashingEmbedding, MarkdownChunker, SectionLocation,
};
use raggy_storage::{Document, Folder, FolderStatus, IndexOwner, Storage};

use crate::error::{CoreError, Result};

/// Opciones de indexación: chunking, embeddings y extracción de documentos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexOptions {
    /// Parámetros de división en fragmentos.
    pub chunking: ChunkingOptions,
    /// Dimensionalidad de los embeddings locales.
    pub embedding_dimensions: usize,
    /// Opciones de extracción (OCR, idiomas, umbral de texto).
    pub extract: ExtractOptions,
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            chunking: ChunkingOptions::default(),
            embedding_dimensions: raggy_rag::DEFAULT_EMBEDDING_DIMENSIONS,
            extract: ExtractOptions::default(),
        }
    }
}

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
    /// Archivos indexables encontrados.
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
    /// Avisos no fatales de la indexación (p. ej. OCR no disponible).
    pub warnings: Vec<String>,
}

/// Extensiones de Markdown que procesa el chunker estructural.
pub const MARKDOWN_EXTENSIONS: &[&str] = &["md", "markdown", "mdx"];

/// ¿La ruta es un documento o código que se puede indexar?
pub fn is_indexable(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    let extension = extension.to_ascii_lowercase();
    is_markdown_extension(&extension)
        || raggy_docs::DocKind::from_extension(&extension).is_some()
        || raggy_llm::is_code_extension(&extension)
}

/// ¿La extensión corresponde a Markdown?
fn is_markdown_extension(extension: &str) -> bool {
    MARKDOWN_EXTENSIONS.contains(&extension)
}

/// Recorre recursivamente `root` y devuelve los archivos indexables.
pub fn scan_indexable_files(root: &Path) -> Vec<PathBuf> {
    scan_indexable_files_checked(root).0
}

/// Recorre la carpeta y devuelve los archivos, si la enumeración fue completa,
/// y avisos de errores. Un recorrido incompleto no se puede usar para borrar
/// del índice archivos que no se alcanzaron a ver.
fn scan_indexable_files_checked(root: &Path) -> (Vec<PathBuf>, bool, Vec<String>) {
    let mut files = Vec::new();
    let mut complete = true;
    let mut warnings = Vec::new();

    for entry in WalkDir::new(root).follow_links(false) {
        match entry {
            Ok(entry) if entry.file_type().is_file() && is_indexable(entry.path()) => {
                files.push(entry.into_path());
            }
            Ok(_) => {}
            Err(error) => {
                complete = false;
                let warning = format!("no se pudo recorrer una ruta: {error}");
                tracing::warn!("{warning}");
                warnings.push(warning);
            }
        }
    }

    (files, complete, warnings)
}

/// Calcula una huella estable de las opciones y herramientas que pueden
/// cambiar el contenido extraído o los embeddings. Al instalar Tesseract,
/// Poppler o LibreOffice después de una indexación fallida, la huella cambia y
/// el siguiente reindexado vuelve a procesar esos archivos.
fn processing_fingerprint(options: &IndexOptions) -> String {
    let mut tools = Tools::detect_with_timeout(options.extract.process_timeout_secs);
    tools.languages.sort();
    let path_label = |path: &Option<PathBuf>| {
        path.as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "ausente".to_string())
    };
    let libreoffice = find_libreoffice()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "ausente".to_string());
    let embedding = HashingEmbedding::new(options.embedding_dimensions);
    let descriptor = format!(
        "pipeline={INDEX_PIPELINE_VERSION};chunk_size={};chunk_overlap={};embedding_name={};embedding_dimensions={};ocr_enabled={};ocr_languages={};min_text_chars={};max_file_bytes={};max_archive_bytes={};max_pages={};max_pdf_page_text_bytes={};max_pdf_text_bytes={};process_timeout_secs={};max_ocr_image_pixels={};tesseract={};tesseract_languages={};pdftoppm={};libreoffice={}",
        options.chunking.max_chars,
        options.chunking.overlap_chars,
        embedding.name(),
        embedding.dimensions(),
        options.extract.ocr_enabled,
        options.extract.ocr_languages.join(","),
        options.extract.min_text_chars,
        options.extract.max_file_bytes,
        options.extract.max_archive_bytes,
        options.extract.max_pages,
        options.extract.max_pdf_page_text_bytes,
        options.extract.max_pdf_text_bytes,
        options.extract.process_timeout_secs,
        options.extract.max_ocr_image_pixels,
        path_label(&tools.tesseract),
        tools.languages.join(","),
        path_label(&tools.pdftoppm),
        libreoffice,
    );
    blake3::hash(descriptor.as_bytes()).to_hex().to_string()
}

/// Extensiones indexables, para los filtros de los diálogos.
///
/// Se derivan de las mismas fuentes que [`is_indexable`] (Markdown, documentos y
/// código) y se ordenan y deduplican, de modo que el filtro nunca se queda
/// desincronizado del motor de indexación.
pub fn indexable_extensions() -> Vec<&'static str> {
    let mut extensions: Vec<&'static str> = MARKDOWN_EXTENSIONS
        .iter()
        .chain(raggy_docs::DOCUMENT_EXTENSIONS)
        .chain(raggy_llm::KNOWN_EXTENSIONS)
        .copied()
        .collect();
    extensions.sort_unstable();
    extensions.dedup();
    extensions
}

/// Incrementar cuando cambie la semántica del pipeline (extractores, chunkers,
/// tokenización o embeddings) sin que cambien las opciones visibles. Al subirlo,
/// la huella de procesamiento cambia y las carpetas se reindexan solas.
const INDEX_PIPELINE_VERSION: u32 = 2;

/// Indexa una carpeta de forma incremental.
///
/// `report` recibe el progreso para poder mostrar una barra en la interfaz.
pub fn index_folder(
    storage: &mut Storage,
    folder_id: &str,
    root: &Path,
    options: &IndexOptions,
    report: &mut dyn FnMut(IndexProgress),
) -> Result<IndexSummary> {
    if !root.is_dir() {
        return Err(CoreError::Index {
            path: root.to_path_buf(),
            message: "la carpeta no existe o no es accesible".to_string(),
        });
    }

    storage.mark_folder_indexing(folder_id)?;

    let (mut files, scan_complete, scan_warnings) = scan_indexable_files_checked(root);
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
    let embedder = HashingEmbedding::new(options.embedding_dimensions);
    let mut summary = IndexSummary {
        folder_id: folder_id.to_string(),
        files_total: total,
        warnings: scan_warnings,
        ..IndexSummary::default()
    };
    let mut seen: HashSet<String> = HashSet::new();
    let processing_hash = processing_fingerprint(options);

    for (position, path) in files.iter().enumerate() {
        let relative = relative_path(root, path);
        seen.insert(relative.clone());

        report(IndexProgress {
            folder_id: folder_id.to_string(),
            phase: IndexPhase::Indexing,
            done: position,
            total,
        });

        let bytes = match read_file_limited(path, options.extract.max_file_bytes) {
            Ok(bytes) => bytes,
            Err(error) => {
                let warning = if error.kind() == std::io::ErrorKind::InvalidData {
                    format!("`{relative}` se omitió: {error}")
                } else {
                    format!("`{relative}` no se pudo leer y se omitió: {error}")
                };
                tracing::warn!(path = %path.display(), "{warning}");
                summary.warnings.push(warning);
                summary.files_failed += 1;
                continue;
            }
        };

        let metadata = std::fs::metadata(path).ok();
        let size_bytes = metadata.as_ref().map_or(0, |m| m.len() as i64);
        let modified_at = metadata.as_ref().map_or(0, modified_millis);
        // El hash se calcula sobre los bytes del archivo: así se puede saltar la
        // extracción (y el OCR, en fases posteriores) si el documento no cambió.
        let content_hash = blake3::hash(&bytes).to_hex().to_string();

        if existing.get(&relative).is_some_and(|document| {
            document.content_hash == content_hash && document.processing_hash == processing_hash
        }) {
            summary.files_skipped += 1;
            continue;
        }

        let (chunks, warnings) = match extract_chunks(path, &relative, &bytes, &chunker, options) {
            Ok(result) => result,
            Err(error) => {
                let warning = format!("`{relative}` no se pudo extraer y se omitió: {error}");
                tracing::warn!(path = %path.display(), "{warning}");
                summary.warnings.push(warning);
                summary.files_failed += 1;
                continue;
            }
        };
        for warning in &warnings {
            tracing::warn!(path = %path.display(), "{warning}");
        }
        summary.warnings.extend(warnings);

        // El encabezado se incluye en el embedding para que una consulta que
        // mencione el título del documento se acerque más a sus fragmentos.
        let embeddings: Vec<Vec<f32>> = chunks
            .iter()
            .map(|chunk| embedder.embed(&embedding_text(chunk)).vector)
            .collect();

        let document = Document::new(
            folder_id,
            relative.clone(),
            content_hash,
            processing_hash.clone(),
            size_bytes,
            modified_at,
            chunks.len() as i64,
        );
        let owner = IndexOwner::Folder(folder_id);
        storage.replace_file_index(owner, &relative, &chunks, &embeddings, &document)?;

        summary.files_indexed += 1;
        summary.chunks_written += chunks.len();
    }

    if scan_complete {
        for relative in existing.keys() {
            if !seen.contains(relative) {
                storage.delete_chunks_for_path(IndexOwner::Folder(folder_id), relative)?;
                storage.delete_document(IndexOwner::Folder(folder_id), relative)?;
                summary.files_removed += 1;
            }
        }
    } else {
        tracing::warn!(
            folder_id,
            "recorrido incompleto; se conservan registros no vistos"
        );
    }

    let indexed_files = storage.list_documents(folder_id)?.len() as i64;
    storage.set_folder_status(folder_id, FolderStatus::Indexed, indexed_files)?;

    report(IndexProgress {
        folder_id: folder_id.to_string(),
        phase: IndexPhase::Done,
        done: total,
        total,
    });

    Ok(summary)
}

/// Lee como máximo `max_bytes + 1` bytes, para aplicar el límite a todos los
/// formatos, incluidos Markdown, texto y código que usan su propio chunker.
fn read_file_limited(path: &Path, max_bytes: u64) -> std::io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("el archivo supera el límite configurado de {max_bytes} bytes"),
        ));
    }
    Ok(bytes)
}

/// Texto que se vectoriza: el encabezado precede al contenido para que una
/// consulta que mencione el título del documento se acerque a sus fragmentos.
fn embedding_text(chunk: &Chunk) -> String {
    match &chunk.metadata.heading {
        Some(heading) => format!("{heading}\n{}", chunk.content),
        None => chunk.content.clone(),
    }
}

/// ¿El archivo se trocea con el chunker de Markdown (encabezados y bloques de
/// código) en lugar del chunker de documentos extraídos?
fn uses_markdown_chunker(path: &Path) -> bool {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) => {
            let extension = extension.to_ascii_lowercase();
            is_markdown_extension(&extension) || raggy_llm::is_code_extension(&extension)
        }
        None => false,
    }
}

/// Obtiene los fragmentos de un archivo: Markdown y código pasan por el chunker
/// semántico; el resto se extrae con `raggy-docs` y se trocea por secciones.
/// Devuelve además los avisos no fatales de la extracción (p. ej. OCR).
fn extract_chunks(
    path: &Path,
    relative: &str,
    bytes: &[u8],
    chunker: &MarkdownChunker,
    options: &IndexOptions,
) -> Result<(Vec<Chunk>, Vec<String>)> {
    if uses_markdown_chunker(path) {
        let content = String::from_utf8_lossy(bytes);
        let source = ChunkSource {
            relative_path: relative,
            content: &content,
        };
        return Ok((chunker.chunk(&source, &options.chunking), Vec::new()));
    }

    let document = raggy_docs::extract_bytes(path, bytes, &options.extract).map_err(|error| {
        CoreError::Index {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })?;
    let warnings = document.warnings;

    let sections: Vec<DocumentSection> = document
        .sections
        .iter()
        .map(|section| {
            DocumentSection::new(
                section.heading.clone(),
                Some(section_location(&section.location)),
                section.text.clone(),
            )
        })
        .collect();

    Ok((
        chunk_sections(relative, &sections, &options.chunking),
        warnings,
    ))
}

/// Traduce la ubicación de una sección de `raggy-docs` al modelo del chunker.
fn section_location(location: &raggy_docs::Location) -> SectionLocation {
    match location {
        raggy_docs::Location::Lines { start, .. } => SectionLocation::Lines { start: *start },
        raggy_docs::Location::Rows {
            sheet,
            columns,
            first_row,
            prefix_lines,
            ..
        } => SectionLocation::Rows {
            sheet: sheet.clone(),
            columns: columns.clone(),
            first_row: *first_row,
            prefix_lines: *prefix_lines,
        },
        raggy_docs::Location::Slide(number) => SectionLocation::Slide(*number),
        raggy_docs::Location::Page(number) => SectionLocation::Page(*number),
    }
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
        /// Opciones de indexación vigentes.
        options: IndexOptions,
    },
    /// Reindexa todas las carpetas registradas.
    ReindexAll {
        /// Opciones de indexación vigentes.
        options: IndexOptions,
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
            .name("raggy-indexer".to_string())
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
    pub fn index_folder(&self, folder_id: impl Into<String>, root: PathBuf, options: IndexOptions) {
        let _ = self.command_tx.send(IndexCommand::IndexFolder {
            folder_id: folder_id.into(),
            root,
            options,
        });
    }

    /// Encola la reindexación de todas las carpetas.
    pub fn reindex_all(&self, options: IndexOptions) {
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
    options: &IndexOptions,
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

    struct TemporalDir {
        path: PathBuf,
    }

    impl TemporalDir {
        fn new(tag: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!("raggy-index-{tag}-{}", std::process::id()));
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

    fn options() -> IndexOptions {
        IndexOptions {
            chunking: ChunkingOptions {
                max_chars: 200,
                overlap_chars: 20,
            },
            embedding_dimensions: 128,
            extract: ExtractOptions::default(),
        }
    }

    #[test]
    fn indexa_markdown_y_archivos_de_forma_recursiva() {
        let dir = TemporalDir::new("recursivo");
        dir.write("uno.md", "# Uno\n\ntexto uno\n");
        dir.write("sub/dos.md", "# Dos\n\ntexto dos\n");
        dir.write("notas.txt", "notas sueltas");
        dir.write("sub/tres.markdown", "documento");
        dir.write("imagen.png", "no es texto");

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

        // Markdown, texto y código sí; la imagen no.
        assert_eq!(summary.files_total, 4);
        assert_eq!(summary.files_indexed, 4);
        assert_eq!(storage.list_documents(&folder.id).expect("lista").len(), 4);
        assert!(storage.count_chunks(&folder.id).expect("cuenta") >= 4);
    }

    #[test]
    fn reconoce_markdown_y_codigo_pero_no_otros_archivos() {
        assert!(is_indexable(Path::new("notas.md")));
        assert!(is_indexable(Path::new("README.markdown")));
        assert!(is_indexable(Path::new("guion.py")));
        assert!(is_indexable(Path::new("main.RS")));
        assert!(is_indexable(Path::new("lib.cpp")));
        assert!(is_indexable(Path::new("cabecera.h")));
        assert!(is_indexable(Path::new("tabla.csv")));
        assert!(is_indexable(Path::new("TABLA.CSV")));
        assert!(is_indexable(Path::new("informe.docx")));
        assert!(is_indexable(Path::new("libro.xlsx")));
        assert!(is_indexable(Path::new("antiguo.xls")));
        assert!(is_indexable(Path::new("charla.pptx")));
        assert!(is_indexable(Path::new("informe.pdf")));
        assert!(is_indexable(Path::new("antiguo.doc")));
        assert!(is_indexable(Path::new("presentacion.ppt")));

        assert!(!is_indexable(Path::new("foto.png")));
        assert!(!is_indexable(Path::new("video.mp4")));
        assert!(!is_indexable(Path::new("binario")));
    }

    #[test]
    fn el_filtro_de_dialogos_coincide_con_el_motor_de_indexacion() {
        let extensions = indexable_extensions();

        // Todo lo que ofrece el filtro debe poder indexarse.
        for extension in &extensions {
            assert!(
                is_indexable(Path::new(&format!("archivo.{extension}"))),
                "«{extension}» está en el filtro pero no se indexa"
            );
        }

        // Extensiones que antes se quedaban fuera del filtro y ahora sí aparecen.
        for extension in [
            "md", "markdown", "mdx", "pdf", "docx", "xlsx", "rs", "py", "less", "astro", "htm",
        ] {
            assert!(
                extensions.contains(&extension),
                "falta «{extension}» en el filtro de diálogos"
            );
        }

        assert!(!extensions.contains(&"rtf"));
    }

    #[test]
    fn indexa_csv_como_secciones_con_ubicacion() {
        let dir = TemporalDir::new("csv");
        dir.write("datos.csv", "nombre,edad\nAna,30\nLuis,25\n");

        let mut storage = Storage::open_in_memory().expect("abre");
        let folder = Folder::new(dir.path.to_string_lossy().to_string(), "datos");
        storage.upsert_folder(&folder).expect("guarda");

        let mut sin_progreso = |_| {};
        let summary = index_folder(
            &mut storage,
            &folder.id,
            &dir.path,
            &options(),
            &mut sin_progreso,
        )
        .expect("indexa");

        assert_eq!(summary.files_indexed, 1);
        let chunks = storage
            .list_chunks_for_path(&folder.id, "datos.csv")
            .expect("lista");
        assert!(!chunks.is_empty());
        assert_eq!(
            chunks[0].metadata.location.as_deref(),
            Some("hoja 1, A2:B3"),
            "el CSV debe citar hoja y coordenadas"
        );
        assert!(chunks[0].content.contains("Ana"));
    }

    #[test]
    fn reporta_y_omite_archivos_que_superan_el_limite_de_ingesta() {
        let dir = TemporalDir::new("limite");
        dir.write("demasiado.txt", "este archivo excede el límite");

        let mut storage = Storage::open_in_memory().expect("abre");
        let folder = Folder::new(dir.path.to_string_lossy().to_string(), "docs");
        storage.upsert_folder(&folder).expect("guarda");

        let mut options = options();
        options.extract.max_file_bytes = 4;
        let mut sin_progreso = |_| {};
        let summary = index_folder(
            &mut storage,
            &folder.id,
            &dir.path,
            &options,
            &mut sin_progreso,
        )
        .expect("la indexación continúa con un aviso");

        assert_eq!(summary.files_failed, 1);
        assert!(summary.warnings[0].contains("supera el límite"));
        assert!(storage
            .list_documents(&folder.id)
            .expect("lista")
            .is_empty());
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
    fn la_reindexacion_aplica_cambios_de_opciones_a_archivos_inalterados() {
        let dir = TemporalDir::new("opciones");
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
        .expect("primera indexación");
        assert_eq!(first.files_indexed, 1);
        let previous = storage
            .get_document(&folder.id, "uno.md")
            .expect("lee documento")
            .expect("existe")
            .processing_hash;

        let mut changed = options();
        changed.chunking.max_chars += 1;
        let second = index_folder(
            &mut storage,
            &folder.id,
            &dir.path,
            &changed,
            &mut sin_progreso,
        )
        .expect("reindexa con nuevas opciones");

        assert_eq!(second.files_indexed, 1);
        assert_eq!(second.files_skipped, 0);
        let current = storage
            .get_document(&folder.id, "uno.md")
            .expect("lee documento actualizado")
            .expect("sigue existiendo");
        assert_ne!(current.processing_hash, previous);
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
