//! Extracción de presentaciones de PowerPoint (`.pptx`).
//!
//! Un `.pptx` es un ZIP con XML; cada diapositiva vive en `ppt/slides/slideN.xml`
//! y su texto en elementos `<a:t>` agrupados por párrafos `<a:p>`. Se usa el
//! marcador de posición de título (`<p:ph type="title"/>`) para poner el título
//! como encabezado de la sección. El formato legacy `.ppt` necesita LibreOffice y
//! se aborda en una fase posterior.

use std::io::{Cursor, Read};

use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;

use crate::{DocKind, ExtractError, ExtractedDocument, Location, Result, Section};

/// Extrae las diapositivas de un `.pptx` a partir de sus bytes.
pub fn extract(bytes: &[u8]) -> Result<ExtractedDocument> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(zip_error)?;
    let mut slide_names = slide_file_names(&mut archive);

    // Orden natural por número de diapositiva.
    slide_names.sort_by_key(|name| slide_number(name));

    let mut sections = Vec::new();
    for (index, name) in slide_names.iter().enumerate() {
        let mut xml = String::new();
        archive
            .by_name(name)
            .map_err(zip_error)?
            .read_to_string(&mut xml)
            .map_err(|error| ExtractError::Parse(error.to_string()))?;

        let (title, lines) = parse_slide(&xml)?;
        let number = (index + 1) as u32;
        let text = lines.join("\n");
        if title.is_none() && text.trim().is_empty() {
            continue;
        }

        sections.push(Section {
            heading: Some(title.unwrap_or_else(|| format!("Diapositiva {number}"))),
            location: Location::Slide(number),
            text,
        });
    }

    Ok(ExtractedDocument {
        kind: DocKind::PowerPoint,
        sections,
    })
}

/// Devuelve los nombres de las entradas de diapositivas del ZIP.
fn slide_file_names<R: Read + std::io::Seek>(archive: &mut zip::ZipArchive<R>) -> Vec<String> {
    let mut names = Vec::new();
    for index in 0..archive.len() {
        if let Ok(entry) = archive.by_index(index) {
            if let Ok(name) = entry.name() {
                let name = name.to_string();
                if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
                    names.push(name);
                }
            }
        }
    }
    names
}

/// Número de diapositiva a partir de `ppt/slides/slideN.xml`.
fn slide_number(name: &str) -> u32 {
    name.trim_end_matches(".xml")
        .rsplit(|character: char| !character.is_ascii_digit())
        .next()
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(0)
}

/// Interpreta una diapositiva: devuelve el título (si lo hay) y las líneas de texto.
fn parse_slide(xml: &str) -> Result<(Option<String>, Vec<String>)> {
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut title: Option<String> = None;
    let mut lines: Vec<String> = Vec::new();
    let mut paragraph = String::new();
    let mut in_text = false;
    let mut title_pending = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Err(error) => return Err(ExtractError::Parse(error.to_string())),
            Ok(Event::Eof) => break,
            Ok(Event::Start(element)) | Ok(Event::Empty(element)) => {
                match element.local_name().as_ref() {
                    b"t" => in_text = true,
                    b"ph" => {
                        let kind = attribute(&element, b"type");
                        if kind == "title" || kind == "ctrTitle" {
                            title_pending = true;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(node)) => {
                if in_text {
                    if let Ok(value) = node.decode() {
                        if let Ok(unescaped) = quick_xml::escape::unescape(&value) {
                            paragraph.push_str(&unescaped);
                        }
                    }
                }
            }
            Ok(Event::End(element)) => match element.local_name().as_ref() {
                b"t" => in_text = false,
                b"p" => {
                    let text = paragraph.trim().to_string();
                    paragraph.clear();
                    if text.is_empty() {
                        continue;
                    }
                    if title_pending && title.is_none() {
                        title = Some(text);
                    } else {
                        lines.push(text);
                    }
                    title_pending = false;
                }
                _ => {}
            },
            _ => {}
        }
        buf.clear();
    }

    Ok((title, lines))
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

/// Traduce un error del ZIP.
fn zip_error(error: zip::result::ZipError) -> ExtractError {
    ExtractError::Parse(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Construye un `.pptx` mínimo con las diapositivas indicadas.
    fn make_pptx(slides: &[String]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (index, body) in slides.iter().enumerate() {
            writer
                .start_file(format!("ppt/slides/slide{}.xml", index + 1), options)
                .unwrap();
            write!(
                writer,
                "<?xml version=\"1.0\"?><p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:cSld><p:spTree>{body}</p:spTree></p:cSld></p:sld>"
            )
            .unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn title_shape(text: &str) -> String {
        format!(
            "<p:sp><p:nvSpPr><p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>"
        )
    }

    fn body_shape(text: &str) -> String {
        format!("<p:sp><p:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>")
    }

    #[test]
    fn extrae_diapositivas_con_titulo() {
        let slides = vec![
            format!(
                "{}{}",
                title_shape("Introducción"),
                body_shape("Primer punto")
            ),
            format!("{}{}", title_shape("Cierre"), body_shape("Gracias")),
        ];
        let document = extract(&make_pptx(&slides)).expect("extrae");
        assert_eq!(document.kind, DocKind::PowerPoint);
        assert_eq!(document.sections.len(), 2);
        assert_eq!(
            document.sections[0].heading.as_deref(),
            Some("Introducción")
        );
        assert_eq!(document.sections[0].location, Location::Slide(1));
        assert!(document.sections[0].text.contains("Primer punto"));
        assert_eq!(document.sections[1].heading.as_deref(), Some("Cierre"));
        assert_eq!(document.sections[1].location, Location::Slide(2));
    }

    #[test]
    fn sin_marcador_de_titulo_usa_el_numero() {
        let slides = vec![body_shape("Solo contenido")];
        let document = extract(&make_pptx(&slides)).expect("extrae");
        assert_eq!(
            document.sections[0].heading.as_deref(),
            Some("Diapositiva 1")
        );
    }

    #[test]
    fn ordena_por_numero_de_diapositiva() {
        assert_eq!(slide_number("ppt/slides/slide12.xml"), 12);
        assert_eq!(slide_number("ppt/slides/slide2.xml"), 2);
    }
}
