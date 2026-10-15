//! `raggy-docs` — extracción de texto de documentos para el RAG.
//!
//! Convierte documentos distintos de Markdown en una lista de secciones con su
//! ubicación, lista para el chunking y las citas. Cubre texto plano, CSV, Excel
//! (`.xlsx`, `.xls`, `.xlsm`), Word (`.docx`), PowerPoint (`.pptx`) y PDF; los
//! PDF escaneados se reconocen por OCR con Tesseract (rasterizando con Poppler)
//! y los formatos legacy `.doc`/`.ppt` se convierten con LibreOffice si está
//! instalado.
//!
//! El crate no depende del motor RAG ni de la interfaz: solo lee el archivo y
//! devuelve secciones.

pub mod csv;
pub mod excel;
pub mod legacy;
pub mod limits;
pub mod ocr;
pub mod pdf;
pub mod slides;
pub mod text;
pub mod tools;
pub mod word;

use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant};

/// Crea un directorio temporal privado y único; se elimina al salir de scope.
pub(crate) fn temp_directory(prefix: &str) -> std::io::Result<tempfile::TempDir> {
    let prefix = format!("raggy-{prefix}-");
    let mut builder = tempfile::Builder::new();
    builder.prefix(&prefix);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    builder.tempdir()
}

/// Valida la expansión real de todas las entradas de un ZIP antes de pasar el
/// archivo a extractores que lo procesan internamente (p. ej. `calamine`).
pub(crate) fn validate_zip_size(
    bytes: &[u8],
    max_uncompressed_bytes: u64,
    max_entries: usize,
) -> Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|error| ExtractError::Parse(format!("paquete Office inválido: {error}")))?;
    if archive.len() > max_entries {
        return Err(ExtractError::ResourceLimit(format!(
            "el paquete contiene {} entradas; el máximo es {max_entries}",
            archive.len()
        )));
    }

    let mut total = 0u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| ExtractError::Parse(format!("entrada Office inválida: {error}")))?;
        // Se mide lo que realmente descomprime, no solo el tamaño declarado en
        // la cabecera ZIP, para detectar tamaños falsificados.
        let remaining = max_uncompressed_bytes.saturating_sub(total);
        let mut bounded = entry.take(remaining.saturating_add(1));
        let copied = io::copy(&mut bounded, &mut io::sink())
            .map_err(|error| ExtractError::Parse(format!("no se pudo validar el ZIP: {error}")))?;
        total = total.saturating_add(copied);
        if total > max_uncompressed_bytes {
            return Err(ExtractError::ResourceLimit(format!(
                "el paquete Office descomprime más de {max_uncompressed_bytes} bytes"
            )));
        }
    }

    Ok(())
}

/// Ejecuta un proceso con un timeout, sin tuberías bloqueantes. El llamador
/// puede redirigir stdout/stderr a archivos si necesita capturar su salida.
pub(crate) fn run_with_timeout(command: &mut Command, timeout: Duration) -> io::Result<ExitStatus> {
    let mut child = command.spawn()?;
    let started = Instant::now();
    let deadline = started
        .checked_add(timeout)
        .unwrap_or_else(|| started + Duration::from_secs(3600));
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "el proceso excedió el límite de {} segundos",
                        timeout.as_secs()
                    ),
                ));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
    }
}

/// Lee un archivo limitando los bytes realmente recibidos, incluso si crece
/// después de consultar sus metadatos.
pub(crate) fn read_bytes_limited(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path).map_err(|source| ExtractError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| ExtractError::Io {
            path: path.display().to_string(),
            source,
        })?;
    if bytes.len() as u64 > max_bytes {
        return Err(ExtractError::ResourceLimit(format!(
            "`{}` excede el límite configurado de {max_bytes} bytes",
            path.display()
        )));
    }
    Ok(bytes)
}

#[cfg(test)]
mod temporary_directory_tests {
    use super::temp_directory;

    #[test]
    fn temporal_es_unico_y_se_limpia_automaticamente() {
        let first_path = {
            let first = temp_directory("test").expect("crea temporal");
            let path = first.path().to_path_buf();
            assert!(path.is_dir());

            let second = temp_directory("test").expect("crea segundo temporal");
            assert_ne!(path, second.path());
            path
        };

        assert!(
            !first_path.exists(),
            "TempDir debe eliminarse al salir de scope"
        );
    }

    #[cfg(unix)]
    #[test]
    fn temporal_es_privado_en_unix() {
        use std::os::unix::fs::PermissionsExt;

        let directory = temp_directory("permisos").expect("crea temporal");
        let mode = std::fs::metadata(directory.path())
            .expect("lee permisos")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);
    }
}

#[cfg(test)]
mod resource_limit_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn limita_lecturas_de_archivos() {
        let mut file = tempfile::NamedTempFile::new().expect("archivo temporal");
        file.write_all(b"contenido largo").expect("escribe");
        let error = read_bytes_limited(file.path(), 4).expect_err("debe superar el límite");
        assert!(matches!(error, ExtractError::ResourceLimit(_)));
    }

    #[test]
    fn el_despachador_rechaza_entradas_mayores_al_limite() {
        let directory = tempfile::tempdir().expect("directorio temporal");
        let path = directory.path().join("entrada.txt");
        std::fs::write(&path, b"demasiado largo").expect("escribe archivo");
        let options = ExtractOptions {
            max_file_bytes: 4,
            ..ExtractOptions::default()
        };
        let error = extract(&path, &options).expect_err("rechaza por tamaño");
        assert!(matches!(error, ExtractError::ResourceLimit(_)));
    }

    #[test]
    fn limita_la_expansion_real_de_archivos_zip() {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .expect("abre entrada");
        writer
            .write_all(&vec![b'x'; 1024])
            .expect("escribe entrada");
        let bytes = writer.finish().expect("cierra zip").into_inner();

        let error = validate_zip_size(&bytes, 64, limits::MAX_ARCHIVE_ENTRIES)
            .expect_err("debe superar el límite");
        assert!(matches!(error, ExtractError::ResourceLimit(_)));
    }

    #[test]
    fn limita_la_cantidad_de_entradas_zip() {
        use std::io::Write;

        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for name in ["uno.xml", "dos.xml"] {
            writer.start_file(name, options).expect("abre entrada");
            writer.write_all(b"x").expect("escribe entrada");
        }
        let bytes = writer.finish().expect("cierra ZIP").into_inner();
        let error = validate_zip_size(&bytes, 1024, 1).expect_err("excede el número permitido");
        assert!(matches!(error, ExtractError::ResourceLimit(_)));
    }

    #[cfg(unix)]
    #[test]
    fn termina_un_proceso_que_excede_el_timeout() {
        let mut command = Command::new("sleep");
        command.arg("2");
        let error = run_with_timeout(&mut command, Duration::from_millis(30))
            .expect_err("el proceso debe agotar su tiempo");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }
}

/// Error de extracción de un documento.
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    /// No se pudo leer el archivo.
    #[error("no se pudo leer `{path}`: {source}")]
    Io {
        /// Ruta afectada.
        path: String,
        /// Causa original.
        #[source]
        source: std::io::Error,
    },

    /// La extensión no tiene extractor en esta versión.
    #[error("formato no soportado todavía: `{0}`")]
    Unsupported(String),

    /// El documento está dañado o no se puede interpretar.
    #[error("no se pudo interpretar el documento: {0}")]
    Parse(String),

    /// El documento excede un límite de recursos configurado.
    #[error("se superó el límite de recursos: {0}")]
    ResourceLimit(String),
}

/// Alias de resultado del crate.
pub type Result<T> = std::result::Result<T, ExtractError>;

/// Tipo de documento reconocido.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocKind {
    /// Texto plano.
    Text,
    /// Valores separados por comas.
    Csv,
    /// Hoja de cálculo de Excel.
    Excel,
    /// Documento de Word.
    Word,
    /// Presentación de PowerPoint.
    PowerPoint,
    /// Documento PDF.
    Pdf,
    /// Documento de Word en formato legacy (`.doc`), requiere LibreOffice.
    WordLegacy,
    /// Presentación en formato legacy (`.ppt`), requiere LibreOffice.
    PresentationLegacy,
}

impl DocKind {
    /// Deduce el tipo a partir de la extensión (en minúsculas), si se reconoce.
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "txt" => Some(Self::Text),
            "csv" => Some(Self::Csv),
            "xlsx" | "xlsm" | "xlsb" | "xls" => Some(Self::Excel),
            "docx" => Some(Self::Word),
            "pptx" => Some(Self::PowerPoint),
            "pdf" => Some(Self::Pdf),
            "doc" => Some(Self::WordLegacy),
            "ppt" => Some(Self::PresentationLegacy),
            _ => None,
        }
    }
}

/// Extensiones de documento que reconoce el extractor. Es la única fuente de
/// verdad: `DocKind::from_extension` y los filtros de la interfaz se derivan de
/// aquí para que no se desincronicen.
pub const DOCUMENT_EXTENSIONS: &[&str] = &[
    "txt", "csv", "xlsx", "xlsm", "xlsb", "xls", "docx", "pptx", "pdf", "doc", "ppt",
];

/// Ubicación de una sección dentro del documento original.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// Rango de líneas del texto extraído (1-based).
    Lines {
        /// Primera línea incluida.
        start: usize,
        /// Última línea incluida.
        end: usize,
    },
    /// Filas de una hoja de cálculo o de un CSV.
    Rows {
        /// Nombre de la hoja; `None` para CSV (se cita como «hoja 1»).
        sheet: Option<String>,
        /// Rango de columnas en letras, p. ej. `A:D`.
        columns: Option<String>,
        /// Número de la primera fila de datos (1-based).
        first_row: usize,
        /// Número de la última fila de datos (1-based).
        last_row: usize,
        /// Líneas iniciales de la sección que no son filas (p. ej. cabecera).
        prefix_lines: usize,
    },
    /// Número de diapositiva de una presentación (1-based).
    Slide(u32),
    /// Número de página de un PDF (1-based).
    Page(u32),
}

impl Location {
    /// Etiqueta legible que se usa en las citas del chat y la búsqueda.
    pub fn label(&self) -> String {
        match self {
            Location::Lines { start, end } => format!("líneas {start}-{end}"),
            Location::Rows {
                sheet,
                columns,
                first_row,
                last_row,
                ..
            } => {
                let sheet = sheet.clone().unwrap_or_else(|| "1".to_string());
                match columns
                    .as_deref()
                    .and_then(|columns| columns.split_once(':'))
                {
                    Some((first, last)) => {
                        format!("hoja {sheet}, {first}{first_row}:{last}{last_row}")
                    }
                    None => format!("hoja {sheet}, filas {first_row}-{last_row}"),
                }
            }
            Location::Slide(number) => format!("diapositiva {number}"),
            Location::Page(number) => format!("página {number}"),
        }
    }
}

/// Letra de columna de una hoja de cálculo a partir del índice (0 → `A`).
pub(crate) fn column_letter(index: usize) -> String {
    let mut index = index + 1;
    let mut letters = String::new();
    while index > 0 {
        let remainder = (index - 1) % 26;
        letters.insert(0, (b'A' + remainder as u8) as char);
        index = (index - 1) / 26;
    }
    letters
}

/// Sección extraída de un documento, lista para trocear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Encabezado o contexto de la sección, si se conoce.
    pub heading: Option<String>,
    /// Ubicación en el documento original.
    pub location: Location,
    /// Texto de la sección.
    pub text: String,
}

/// Documento extraído: su tipo, sus secciones y los avisos de la extracción.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedDocument {
    /// Tipo de documento.
    pub kind: DocKind,
    /// Secciones en orden de aparición.
    pub sections: Vec<Section>,
    /// Avisos no fatales (p. ej. OCR no disponible).
    pub warnings: Vec<String>,
}

impl ExtractedDocument {
    /// Crea un documento sin avisos.
    pub fn new(kind: DocKind, sections: Vec<Section>) -> Self {
        Self {
            kind,
            sections,
            warnings: Vec::new(),
        }
    }

    /// ¿El documento no produjo ningún texto?
    pub fn is_empty(&self) -> bool {
        self.sections
            .iter()
            .all(|section| section.text.trim().is_empty())
    }
}

/// Opciones de extracción, incluidas las del OCR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractOptions {
    /// ¿Se intenta OCR cuando el documento no tiene texto?
    pub ocr_enabled: bool,
    /// Idiomas de OCR, en orden de preferencia (p. ej. `["spa"]`).
    pub ocr_languages: Vec<String>,
    /// Texto mínimo de una página para considerar que tiene capa de texto.
    pub min_text_chars: usize,
    /// Tamaño máximo de entrada, en bytes.
    pub max_file_bytes: u64,
    /// Máximo total descomprimido para documentos Office ZIP.
    pub max_archive_bytes: u64,
    /// Máximo de páginas PDF a extraer.
    pub max_pages: usize,
    /// Máximo de bytes de texto conservado por página PDF.
    pub max_pdf_page_text_bytes: u64,
    /// Máximo total de bytes de texto conservado por documento PDF.
    pub max_pdf_text_bytes: u64,
    /// Tiempo máximo de espera de cada herramienta externa, en segundos.
    pub process_timeout_secs: u64,
    /// Máximo de píxeles por página durante el OCR.
    pub max_ocr_image_pixels: u64,
}

impl Default for ExtractOptions {
    fn default() -> Self {
        Self {
            ocr_enabled: true,
            ocr_languages: vec!["spa".to_string()],
            min_text_chars: limits::DEFAULT_MIN_TEXT_CHARS,
            max_file_bytes: limits::DEFAULT_MAX_FILE_MIB * limits::BYTES_PER_MIB,
            max_archive_bytes: limits::DEFAULT_MAX_ARCHIVE_MIB * limits::BYTES_PER_MIB,
            max_pages: limits::DEFAULT_MAX_PAGES,
            max_pdf_page_text_bytes: limits::DEFAULT_MAX_PDF_PAGE_TEXT_MIB * limits::BYTES_PER_MIB,
            max_pdf_text_bytes: limits::DEFAULT_MAX_PDF_TEXT_MIB * limits::BYTES_PER_MIB,
            process_timeout_secs: limits::DEFAULT_PROCESS_TIMEOUT_SECS,
            max_ocr_image_pixels: limits::DEFAULT_MAX_OCR_IMAGE_PIXELS,
        }
    }
}

/// Extrae el texto de `path` según su extensión.
pub fn extract(path: &Path, options: &ExtractOptions) -> Result<ExtractedDocument> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    if DocKind::from_extension(extension).is_none() {
        return Err(ExtractError::Unsupported(extension.to_ascii_lowercase()));
    }
    let bytes = read_bytes_limited(path, options.max_file_bytes)?;
    extract_bytes(path, &bytes, options)
}

/// Extrae desde los bytes ya leídos por el indexador. Así el hash incremental y
/// el contenido extraído siempre corresponden a la misma versión del archivo.
pub fn extract_bytes(
    path: &Path,
    bytes: &[u8],
    options: &ExtractOptions,
) -> Result<ExtractedDocument> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let kind = DocKind::from_extension(&extension)
        .ok_or_else(|| ExtractError::Unsupported(extension.clone()))?;

    if bytes.len() as u64 > options.max_file_bytes {
        return Err(ExtractError::ResourceLimit(format!(
            "`{}` excede el límite configurado de {} bytes",
            path.display(),
            options.max_file_bytes
        )));
    }

    Ok(match kind {
        DocKind::Text => text::extract(bytes),
        DocKind::Csv => csv::extract(bytes)?,
        DocKind::Excel => excel::extract_with_limits(bytes, &extension, options.max_archive_bytes)?,
        DocKind::Word => word::extract_with_limits(bytes, options.max_archive_bytes)?,
        DocKind::PowerPoint => slides::extract_with_limits(bytes, options.max_archive_bytes)?,
        DocKind::Pdf => pdf::extract(path, bytes, options)?,
        DocKind::WordLegacy => legacy::extract(path, bytes, legacy::LegacyTarget::Word, options)?,
        DocKind::PresentationLegacy => {
            legacy::extract(path, bytes, legacy::LegacyTarget::Presentation, options)?
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduce_el_tipo_por_extension() {
        assert_eq!(DocKind::from_extension("txt"), Some(DocKind::Text));
        assert_eq!(DocKind::from_extension("CSV"), Some(DocKind::Csv));
        assert_eq!(DocKind::from_extension("xlsx"), Some(DocKind::Excel));
        assert_eq!(DocKind::from_extension("xls"), Some(DocKind::Excel));
        assert_eq!(DocKind::from_extension("docx"), Some(DocKind::Word));
        assert_eq!(DocKind::from_extension("pptx"), Some(DocKind::PowerPoint));
        assert_eq!(DocKind::from_extension("pdf"), Some(DocKind::Pdf));
        assert_eq!(DocKind::from_extension("doc"), Some(DocKind::WordLegacy));
        assert_eq!(
            DocKind::from_extension("ppt"),
            Some(DocKind::PresentationLegacy)
        );
        assert_eq!(DocKind::from_extension("rtf"), None);
    }

    #[test]
    fn etiqueta_de_ubicacion() {
        assert_eq!(Location::Lines { start: 1, end: 9 }.label(), "líneas 1-9");
        assert_eq!(
            Location::Rows {
                sheet: Some("Ventas".to_string()),
                columns: Some("A:D".to_string()),
                first_row: 2,
                last_row: 20,
                prefix_lines: 1,
            }
            .label(),
            "hoja Ventas, A2:D20"
        );
        assert_eq!(
            Location::Rows {
                sheet: None,
                columns: None,
                first_row: 2,
                last_row: 5,
                prefix_lines: 1,
            }
            .label(),
            "hoja 1, filas 2-5"
        );
        assert_eq!(Location::Slide(3).label(), "diapositiva 3");
        assert_eq!(Location::Page(7).label(), "página 7");
    }

    #[test]
    fn letras_de_columna() {
        assert_eq!(column_letter(0), "A");
        assert_eq!(column_letter(25), "Z");
        assert_eq!(column_letter(26), "AA");
        assert_eq!(column_letter(27), "AB");
    }

    #[test]
    fn opciones_por_defecto_activan_ocr_en_espanol() {
        let options = ExtractOptions::default();
        assert!(options.ocr_enabled);
        assert_eq!(options.ocr_languages, vec!["spa".to_string()]);
    }

    #[test]
    fn extension_no_soportada_da_error() {
        let result = extract(
            Path::new("/tmp/inexistente.rtf"),
            &ExtractOptions::default(),
        );
        assert!(matches!(result, Err(ExtractError::Unsupported(_))));
    }
}
