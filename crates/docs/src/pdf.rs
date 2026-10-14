//! Extracción de PDF. Usa la capa de texto si existe y, cuando una página no
//! tiene texto suficiente, recurre al OCR con Tesseract (rasterizando con
//! Poppler) si las herramientas están disponibles. La extracción se acota para
//! no caer en bombas de descompresión.

use std::path::Path;

use lopdf::Document;

use crate::ocr::Ocr;
use crate::{
    read_bytes, DocKind, ExtractError, ExtractOptions, ExtractedDocument, Location, Result, Section,
};

/// Tope de bytes descomprimidos por página (protección contra bombas).
const MAX_PAGE_BYTES: usize = 64 * 1024 * 1024;

/// Extrae el texto de un PDF, una sección por página.
pub fn extract(path: &Path, options: &ExtractOptions) -> Result<ExtractedDocument> {
    let bytes = read_bytes(path)?;
    let document = Document::load_mem(&bytes).map_err(parse_error)?;

    // Las herramientas de OCR se detectan solo si alguna página las necesita.
    let mut ocr: Option<Ocr> = None;
    let mut sections = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let file_name = file_name(path);

    for (number, _id) in document.get_pages() {
        let mut text = normalize(&page_text(&document, number));

        if text.chars().count() < options.min_text_chars && options.ocr_enabled {
            let ocr = ocr.get_or_insert_with(|| Ocr::detect(&options.ocr_languages));
            if ocr.is_ready() {
                match ocr.recognize_pdf_page(path, number) {
                    Ok(recognized) => {
                        let recognized = normalize(&recognized);
                        if !recognized.is_empty() {
                            text = if text.is_empty() {
                                recognized
                            } else {
                                format!("{text}\n{recognized}")
                            };
                        }
                    }
                    Err(error) => {
                        push_unique(
                            &mut warnings,
                            format!("OCR de la página {number} de `{file_name}` falló: {error}"),
                        );
                    }
                }
            } else if let Some(reason) = ocr.unavailability() {
                push_unique(
                    &mut warnings,
                    format!("`{file_name}` necesita OCR pero no está disponible: {reason}"),
                );
            }
        }

        if !text.is_empty() {
            sections.push(Section {
                heading: None,
                location: Location::Page(number),
                text,
            });
        }
    }

    let mut extracted = ExtractedDocument::new(DocKind::Pdf, sections);
    extracted.warnings = warnings;
    Ok(extracted)
}

/// Texto de una página con la capa existente (sin lanzar error si falla).
fn page_text(document: &Document, number: u32) -> String {
    document
        .extract_text_with_limit(&[number], MAX_PAGE_BYTES)
        .unwrap_or_default()
}

/// Normaliza el texto de una página: recorta líneas y colapsa líneas en blanco.
fn normalize(text: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    let mut previous_blank = true;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !previous_blank {
                lines.push("");
                previous_blank = true;
            }
        } else {
            lines.push(trimmed);
            previous_blank = false;
        }
    }

    while lines.last() == Some(&"") {
        lines.pop();
    }

    lines.join("\n")
}

/// Añade un aviso si no estaba ya.
fn push_unique(warnings: &mut Vec<String>, warning: String) {
    if !warnings.contains(&warning) {
        warnings.push(warning);
    }
}

/// Nombre de archivo a partir de una ruta.
fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("documento.pdf")
        .to_string()
}

/// Traduce un error de lopdf.
fn parse_error(error: lopdf::Error) -> ExtractError {
    ExtractError::Parse(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normaliza_lineas_y_espacios() {
        let raw = "  Título  \n\n\n  Cuerpo con texto  \n\n";
        assert_eq!(normalize(raw), "Título\n\nCuerpo con texto");
    }

    #[test]
    fn un_pdf_invalido_da_error() {
        let options = ExtractOptions::default();
        assert!(extract(Path::new("/tmp/raggy-pdf-inexistente.pdf"), &options).is_err());
    }

    #[test]
    fn los_avisos_no_se_repiten() {
        let mut warnings = Vec::new();
        push_unique(&mut warnings, "aviso".to_string());
        push_unique(&mut warnings, "aviso".to_string());
        assert_eq!(warnings.len(), 1);
    }
}
