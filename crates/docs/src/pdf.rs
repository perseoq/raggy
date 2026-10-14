//! Extracción de PDF con capa de texto.
//!
//! Se apoya en `lopdf` para leer el documento y extraer el texto de cada
//! página. Los PDF escaneados (sin capa de texto) no producen secciones; el OCR
//! se aborda en una fase posterior. La extracción se acota para no caer en
//! bombas de descompresión.

use lopdf::Document;

use crate::{DocKind, ExtractError, ExtractedDocument, Location, Result, Section};

/// Tope de bytes descomprimidos por página (protección contra bombas).
const MAX_PAGE_BYTES: usize = 64 * 1024 * 1024;

/// Extrae el texto de un PDF a partir de sus bytes, una sección por página.
pub fn extract(bytes: &[u8]) -> Result<ExtractedDocument> {
    let document = Document::load_mem(bytes).map_err(parse_error)?;
    let mut sections = Vec::new();

    for (number, _id) in document.get_pages() {
        let text = document
            .extract_text_with_limit(&[number], MAX_PAGE_BYTES)
            .unwrap_or_default();
        let text = normalize(&text);
        if text.is_empty() {
            continue;
        }

        sections.push(Section {
            heading: None,
            location: Location::Page(number),
            text,
        });
    }

    Ok(ExtractedDocument {
        kind: DocKind::Pdf,
        sections,
    })
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
        assert!(extract(b"esto no es un pdf").is_err());
    }
}
