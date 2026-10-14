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
pub mod ocr;
pub mod pdf;
pub mod slides;
pub mod text;
pub mod tools;
pub mod word;

use std::path::{Path, PathBuf};

/// Crea un directorio temporal único con el prefijo indicado.
pub(crate) fn temp_directory(prefix: &str) -> std::io::Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    let directory =
        std::env::temp_dir().join(format!("raggy-{prefix}-{}-{sequence}", std::process::id()));
    std::fs::create_dir_all(&directory)?;
    Ok(directory)
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
}

impl Default for ExtractOptions {
    fn default() -> Self {
        Self {
            ocr_enabled: true,
            ocr_languages: vec!["spa".to_string()],
            min_text_chars: 24,
        }
    }
}

/// Extrae el texto de `path` según su extensión.
pub fn extract(path: &Path, options: &ExtractOptions) -> Result<ExtractedDocument> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let kind = DocKind::from_extension(&extension)
        .ok_or_else(|| ExtractError::Unsupported(extension.clone()))?;

    Ok(match kind {
        DocKind::Text => text::extract(&read_bytes(path)?),
        DocKind::Csv => csv::extract(&read_bytes(path)?)?,
        DocKind::Excel => excel::extract(path)?,
        DocKind::Word => word::extract(&read_bytes(path)?)?,
        DocKind::PowerPoint => slides::extract(&read_bytes(path)?)?,
        DocKind::Pdf => pdf::extract(path, options)?,
        DocKind::WordLegacy => legacy::extract(path, legacy::LegacyTarget::Word)?,
        DocKind::PresentationLegacy => legacy::extract(path, legacy::LegacyTarget::Presentation)?,
    })
}

/// Lee el archivo como bytes, con un error que incluye la ruta.
pub(crate) fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|source| ExtractError::Io {
        path: path.display().to_string(),
        source,
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
