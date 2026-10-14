//! Extracción de texto plano.

use crate::{DocKind, ExtractedDocument, Location, Section};

/// Extrae un archivo de texto como una única sección con rango de líneas.
///
/// Se decodifica de forma tolerante (UTF-8 con reemplazo de bytes inválidos),
/// se normalizan los saltos de línea y el BOM, y se conservan las líneas para
/// poder citar `líneas N-M`.
pub fn extract(bytes: &[u8]) -> ExtractedDocument {
    let text = String::from_utf8_lossy(bytes);
    let normalized = normalize(&text);

    let line_count = normalized.lines().count().max(1);
    let trimmed = normalized.trim();

    let sections = if trimmed.is_empty() {
        Vec::new()
    } else {
        vec![Section {
            heading: None,
            location: Location::Lines {
                start: 1,
                end: line_count,
            },
            text: trimmed.to_string(),
        }]
    };

    ExtractedDocument::new(DocKind::Text, sections)
}

/// Normaliza saltos de línea y elimina el BOM inicial.
fn normalize(text: &str) -> String {
    let without_bom = text.strip_prefix('\u{feff}').unwrap_or(text);
    without_bom.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrae_una_seccion_con_lineas() {
        let document = extract(b"linea uno\r\nlinea dos\r\n");
        assert_eq!(document.kind, DocKind::Text);
        assert_eq!(document.sections.len(), 1);
        assert_eq!(document.sections[0].text, "linea uno\nlinea dos");
        assert_eq!(
            document.sections[0].location,
            Location::Lines { start: 1, end: 2 }
        );
    }

    #[test]
    fn quita_el_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"hola");
        let document = extract(&bytes);
        assert_eq!(document.sections[0].text, "hola");
    }

    #[test]
    fn texto_vacio_no_produce_secciones() {
        assert!(extract(b"   \n\n").sections.is_empty());
    }

    #[test]
    fn bytes_invalidos_no_fallan() {
        let document = extract(&[0xFF, 0xFE, b'h', b'i']);
        assert_eq!(document.sections.len(), 1);
        assert!(document.sections[0].text.contains("hi"));
    }
}
