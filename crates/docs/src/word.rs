//! Extracción de documentos de Word (`.docx`).
//!
//! Un `.docx` es un ZIP con XML; el texto vive en `word/document.xml`, en
//! elementos `<w:t>` agrupados por párrafos `<w:p>`. Se detectan los títulos por
//! el estilo del párrafo (`w:pStyle`, p. ej. `Heading1`) para usarlos como
//! encabezado de sección. Los formatos legacy `.doc` necesitan LibreOffice y se
//! abordan en una fase posterior.

use std::io::{Cursor, Read};

use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;

use crate::{DocKind, ExtractError, ExtractedDocument, Location, Result, Section};

/// Extrae el texto de un `.docx` a partir de sus bytes.
pub fn extract(bytes: &[u8]) -> Result<ExtractedDocument> {
    let xml = read_document_xml(bytes)?;
    let paragraphs = parse_paragraphs(&xml)?;
    Ok(build_document(paragraphs))
}

/// Párrafo extraído, con su nivel de título si lo es.
struct Paragraph {
    text: String,
    heading: Option<u8>,
}

/// Lee `word/document.xml` del archivo ZIP.
fn read_document_xml(bytes: &[u8]) -> Result<String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(zip_error)?;
    let mut file = archive.by_name("word/document.xml").map_err(zip_error)?;
    let mut xml = String::new();
    file.read_to_string(&mut xml)
        .map_err(|error| ExtractError::Parse(error.to_string()))?;
    Ok(xml)
}

/// Recorre el XML y devuelve los párrafos con su texto.
fn parse_paragraphs(xml: &str) -> Result<Vec<Paragraph>> {
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut paragraphs = Vec::new();
    let mut text = String::new();
    let mut heading: Option<u8> = None;
    let mut in_text = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Err(error) => return Err(ExtractError::Parse(error.to_string())),
            Ok(Event::Eof) => break,
            Ok(Event::Start(element)) => match element.local_name().as_ref() {
                b"p" => {
                    text.clear();
                    heading = None;
                }
                b"t" => in_text = true,
                b"pStyle" => heading = style_heading_level(&attribute(&element, b"val")),
                _ => {}
            },
            Ok(Event::Empty(element)) => {
                if element.local_name().as_ref() == b"pStyle" {
                    heading = style_heading_level(&attribute(&element, b"val"));
                }
            }
            Ok(Event::Text(node)) => {
                if in_text {
                    if let Ok(value) = node.decode() {
                        if let Ok(unescaped) = quick_xml::escape::unescape(&value) {
                            text.push_str(&unescaped);
                        }
                    }
                }
            }
            Ok(Event::End(element)) => match element.local_name().as_ref() {
                b"t" => in_text = false,
                b"p" => paragraphs.push(Paragraph {
                    text: text.trim().to_string(),
                    heading,
                }),
                _ => {}
            },
            _ => {}
        }
        buf.clear();
    }

    Ok(paragraphs)
}

/// Devuelve el valor de un atributo del elemento, si existe.
fn attribute(element: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> String {
    for attribute in element.attributes().flatten() {
        if attribute.key.local_name().as_ref() == name {
            return String::from_utf8_lossy(&attribute.value).to_string();
        }
    }
    String::new()
}

/// Interpreta el nombre de un estilo como nivel de título (1-9), si lo es.
fn style_heading_level(style: &str) -> Option<u8> {
    let lower = style.to_ascii_lowercase();
    let is_heading = [
        "heading",
        "título",
        "titulo",
        "berschrift",
        "titre",
        "titolo",
        "kop",
        "overskrift",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix));

    if !is_heading {
        return None;
    }

    lower
        .chars()
        .rev()
        .find_map(|character| character.to_digit(10))
        .map(|level| level.clamp(1, 9) as u8)
        .or(Some(1))
}

/// Agrupa los párrafos en secciones usando los títulos como separadores.
fn build_document(paragraphs: Vec<Paragraph>) -> ExtractedDocument {
    let mut sections = Vec::new();
    let mut heading: Option<String> = None;
    let mut lines: Vec<String> = Vec::new();
    let mut start_line = 1usize;
    let mut last_line = 1usize;
    let mut index = 0usize;

    for paragraph in paragraphs {
        index += 1;
        if let Some(_level) = paragraph.heading {
            if !lines.is_empty() {
                sections.push(make_section(
                    &mut heading,
                    &mut lines,
                    start_line,
                    last_line,
                ));
            }
            let title = paragraph.text.trim().to_string();
            if !title.is_empty() {
                heading = Some(title);
            }
        } else if !paragraph.text.trim().is_empty() {
            if lines.is_empty() {
                start_line = index;
            }
            last_line = index;
            lines.push(paragraph.text.trim().to_string());
        }
    }

    if !lines.is_empty() {
        sections.push(make_section(
            &mut heading,
            &mut lines,
            start_line,
            last_line,
        ));
    }

    ExtractedDocument::new(DocKind::Word, sections)
}

/// Construye una sección a partir de las líneas acumuladas.
fn make_section(
    heading: &mut Option<String>,
    lines: &mut Vec<String>,
    start_line: usize,
    end_line: usize,
) -> Section {
    let text = lines.join("\n");
    lines.clear();
    Section {
        heading: heading.take(),
        location: Location::Lines {
            start: start_line,
            end: end_line.max(start_line),
        },
        text,
    }
}

/// Traduce un error del ZIP.
fn zip_error(error: zip::result::ZipError) -> ExtractError {
    ExtractError::Parse(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Construye un `.docx` mínimo con el cuerpo indicado.
    fn make_docx(body: &str) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("word/document.xml", options).unwrap();
        write!(
            writer,
            "<?xml version=\"1.0\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}</w:body></w:document>"
        )
        .unwrap();
        writer.finish().unwrap().into_inner()
    }

    fn paragraph_with_style(style: &str, text: &str) -> String {
        format!(
            "<w:p><w:pPr><w:pStyle w:val=\"{style}\"/></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"
        )
    }

    fn paragraph(text: &str) -> String {
        format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>")
    }

    #[test]
    fn extrae_parrafos_y_titulos() {
        let body = format!(
            "{}{}{}",
            paragraph_with_style("Heading1", "Introducción"),
            paragraph("Primer párrafo del documento."),
            paragraph("Segundo párrafo."),
        );
        let document = extract(&make_docx(&body)).expect("extrae");
        assert_eq!(document.kind, DocKind::Word);
        assert_eq!(document.sections.len(), 1);
        assert_eq!(
            document.sections[0].heading.as_deref(),
            Some("Introducción")
        );
        assert!(document.sections[0].text.contains("Primer párrafo"));
        assert!(document.sections[0].text.contains("Segundo párrafo"));
    }

    #[test]
    fn sin_estilos_es_una_sola_seccion() {
        let body = format!("{}{}", paragraph("uno"), paragraph("dos"));
        let document = extract(&make_docx(&body)).expect("extrae");
        assert_eq!(document.sections.len(), 1);
        assert_eq!(document.sections[0].heading, None);
    }

    #[test]
    fn varios_titulos_dividen_en_secciones() {
        let body = format!(
            "{}{}{}{}",
            paragraph_with_style("Heading1", "Uno"),
            paragraph("Contenido uno"),
            paragraph_with_style("Heading2", "Dos"),
            paragraph("Contenido dos"),
        );
        let document = extract(&make_docx(&body)).expect("extrae");
        assert_eq!(document.sections.len(), 2);
        assert_eq!(document.sections[0].heading.as_deref(), Some("Uno"));
        assert_eq!(document.sections[1].heading.as_deref(), Some("Dos"));
    }

    #[test]
    fn reconoce_niveles_de_titulo() {
        assert_eq!(style_heading_level("Heading1"), Some(1));
        assert_eq!(style_heading_level("Título2"), Some(2));
        assert_eq!(style_heading_level("Normal"), None);
    }
}
