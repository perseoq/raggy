//! `raggy-docs` — extracción de texto de documentos para el RAG.
//!
//! Convierte documentos distintos de Markdown en una lista de secciones con su
//! ubicación, lista para el chunking y las citas. Cubre texto plano, CSV, Excel
//! (`.xlsx`, `.xls`, `.xlsm`), Word (`.docx`), PowerPoint (`.pptx`) y PDF con
//! capa de texto; las fases siguientes añaden OCR de PDF escaneados y los
//! formatos legacy `.doc`/`.ppt` (vía LibreOffice).
//!
//! El crate no depende del motor RAG ni de la interfaz: solo lee el archivo y
//! devuelve secciones.

pub mod csv;
pub mod excel;
pub mod pdf;
pub mod slides;
pub mod text;
pub mod word;

use std::path::Path;

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
    /// Documento PDF con capa de texto.
    Pdf,
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
    /// Rango de filas de datos de un CSV, sin contar la cabecera (1-based).
    Rows {
        /// Primera fila incluida.
        start: usize,
        /// Última fila incluida.
        end: usize,
    },
    /// Rango de filas de una hoja de cálculo.
    Sheet {
        /// Nombre de la hoja.
        name: String,
        /// Primera fila incluida.
        start: usize,
        /// Última fila incluida.
        end: usize,
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
            Location::Rows { start, end } => format!("filas {start}-{end}"),
            Location::Sheet { name, start, end } => {
                format!("hoja {name}, filas {start}-{end}")
            }
            Location::Slide(number) => format!("diapositiva {number}"),
            Location::Page(number) => format!("página {number}"),
        }
    }
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

/// Documento extraído: su tipo y sus secciones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedDocument {
    /// Tipo de documento.
    pub kind: DocKind,
    /// Secciones en orden de aparición.
    pub sections: Vec<Section>,
}

impl ExtractedDocument {
    /// ¿El documento no produjo ningún texto?
    pub fn is_empty(&self) -> bool {
        self.sections
            .iter()
            .all(|section| section.text.trim().is_empty())
    }
}

/// Opciones de extracción.
///
/// En esta fase no hay ninguna; las fases de PDF y OCR añadirán aquí el idioma
/// de OCR, el número máximo de páginas y si el OCR está activo.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExtractOptions {}

/// Extrae el texto de `path` según su extensión.
pub fn extract(path: &Path, options: &ExtractOptions) -> Result<ExtractedDocument> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let kind = DocKind::from_extension(&extension)
        .ok_or_else(|| ExtractError::Unsupported(extension.clone()))?;

    let _ = options;
    Ok(match kind {
        DocKind::Text => text::extract(&read_bytes(path)?),
        DocKind::Csv => csv::extract(&read_bytes(path)?)?,
        DocKind::Excel => excel::extract(path)?,
        DocKind::Word => word::extract(&read_bytes(path)?)?,
        DocKind::PowerPoint => slides::extract(&read_bytes(path)?)?,
        DocKind::Pdf => pdf::extract(&read_bytes(path)?)?,
    })
}

/// Lee el archivo como bytes, con un error que incluye la ruta.
fn read_bytes(path: &Path) -> Result<Vec<u8>> {
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
        assert_eq!(DocKind::from_extension("rtf"), None);
    }

    #[test]
    fn etiqueta_de_ubicacion() {
        assert_eq!(Location::Lines { start: 1, end: 9 }.label(), "líneas 1-9");
        assert_eq!(Location::Rows { start: 2, end: 5 }.label(), "filas 2-5");
        assert_eq!(
            Location::Sheet {
                name: "Ventas".to_string(),
                start: 1,
                end: 40,
            }
            .label(),
            "hoja Ventas, filas 1-40"
        );
        assert_eq!(Location::Slide(3).label(), "diapositiva 3");
        assert_eq!(Location::Page(7).label(), "página 7");
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
