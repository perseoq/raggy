//! Extracción de PDF. Usa la capa de texto si existe y, cuando una página no
//! tiene texto suficiente, recurre al OCR con Tesseract (rasterizando con
//! Poppler) si las herramientas están disponibles. La extracción se acota para
//! no caer en bombas de descompresión.

use std::path::Path;

use lopdf::Document;

use crate::ocr::Ocr;
use crate::{DocKind, ExtractError, ExtractOptions, ExtractedDocument, Location, Result, Section};

/// Extrae el texto de un PDF, una sección por página.
pub fn extract(path: &Path, bytes: &[u8], options: &ExtractOptions) -> Result<ExtractedDocument> {
    let document = Document::load_mem(bytes).map_err(parse_error)?;

    // Las herramientas de OCR se detectan solo si alguna página las necesita.
    let mut ocr: Option<Ocr> = None;
    let mut sections = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let file_name = file_name(path);
    let pages = document.get_pages();
    if pages.len() > options.max_pages {
        warnings.push(format!(
            "`{file_name}` tiene {} páginas; se procesan solo las primeras {} según el límite configurado.",
            pages.len(),
            options.max_pages
        ));
    }

    let max_page_text_bytes =
        usize::try_from(options.max_pdf_page_text_bytes).unwrap_or(usize::MAX);
    let max_document_text_bytes = usize::try_from(options.max_pdf_text_bytes).unwrap_or(usize::MAX);
    let mut total_text_bytes = 0usize;
    for (number, _id) in pages.into_iter().take(options.max_pages) {
        let remaining = max_document_text_bytes.saturating_sub(total_text_bytes);
        if remaining == 0 {
            push_unique(
                &mut warnings,
                format!(
                    "`{file_name}` excede el límite de texto PDF de {max_document_text_bytes} bytes."
                ),
            );
            break;
        }
        let page_limit = remaining.min(max_page_text_bytes);
        let mut text = normalize(&page_text(&document, number, page_limit));

        if text.chars().count() < options.min_text_chars && options.ocr_enabled {
            let ocr = ocr.get_or_insert_with(|| {
                Ocr::detect(&options.ocr_languages, options.process_timeout_secs)
            });
            if ocr.is_ready() {
                match ocr.recognize_pdf_page(path, number, options.max_ocr_image_pixels) {
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
            if text.len() > page_limit {
                text.truncate(last_char_boundary(&text, page_limit));
                push_unique(
                    &mut warnings,
                    format!(
                        "OCR de una página de `{file_name}` alcanzó el límite de {page_limit} bytes."
                    ),
                );
            }
            if text.len() > remaining {
                text.truncate(last_char_boundary(&text, remaining));
                push_unique(
                    &mut warnings,
                    format!(
                        "`{file_name}` alcanzó el límite de texto PDF de {max_document_text_bytes} bytes."
                    ),
                );
            }
            total_text_bytes = total_text_bytes.saturating_add(text.len());
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
fn page_text(document: &Document, number: u32, max_bytes: usize) -> String {
    document
        .extract_text_with_limit(&[number], max_bytes)
        .unwrap_or_default()
}

/// Encuentra el último límite UTF-8 que no supera `max_bytes`.
fn last_char_boundary(text: &str, max_bytes: usize) -> usize {
    let mut boundary = max_bytes.min(text.len());
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
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

    fn text_pdf(text: &str) -> Vec<u8> {
        use lopdf::{
            content::{Content, Operation},
            dictionary, Object, Stream,
        };

        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let resources = dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        };
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 12.into()]),
                Operation::new("Td", vec![100.into(), 700.into()]),
                Operation::new("Tj", vec![Object::string_literal(text)]),
                Operation::new("ET", vec![]),
            ],
        }
        .encode()
        .expect("codifica contenido PDF");
        let content_id = document.add_object(Stream::new(dictionary! {}, content));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => resources,
            "Contents" => content_id,
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("serializa PDF");
        bytes
    }

    fn blank_pdf(page_count: u32) -> Vec<u8> {
        use lopdf::{dictionary, Document, Object};

        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let mut kids = Vec::new();
        for _ in 0..page_count {
            let page_id = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            });
            kids.push(Object::Reference(page_id));
        }
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => page_count,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("serializa PDF");
        bytes
    }

    #[test]
    fn normaliza_lineas_y_espacios() {
        let raw = "  Título  \n\n\n  Cuerpo con texto  \n\n";
        assert_eq!(normalize(raw), "Título\n\nCuerpo con texto");
    }

    #[test]
    fn un_pdf_invalido_da_error() {
        let options = ExtractOptions::default();
        assert!(extract(Path::new("/tmp/raggy-pdf.pdf"), b"invalid", &options).is_err());
    }

    #[test]
    fn extrae_texto_real_y_cita_la_pagina() {
        let options = ExtractOptions::default();
        let bytes = text_pdf("Raggy PDF verificable");
        let extracted = extract(Path::new("guia.pdf"), &bytes, &options).expect("extrae PDF");
        assert_eq!(extracted.sections.len(), 1);
        assert!(extracted.sections[0].text.contains("Raggy PDF verificable"));
        assert_eq!(extracted.sections[0].location, Location::Page(1));
    }

    #[test]
    fn aplica_los_limites_de_texto_pdf_configurados() {
        let options = ExtractOptions {
            ocr_enabled: false,
            max_pdf_page_text_bytes: 8,
            max_pdf_text_bytes: 8,
            ..ExtractOptions::default()
        };
        let bytes = text_pdf("Este texto es más largo que el límite configurado.");
        let extracted = extract(Path::new("limite.pdf"), &bytes, &options).expect("extrae PDF");
        assert!(extracted
            .sections
            .iter()
            .all(|section| section.text.len() <= 8));
    }

    #[test]
    fn los_avisos_no_se_repiten() {
        let mut warnings = Vec::new();
        push_unique(&mut warnings, "aviso".to_string());
        push_unique(&mut warnings, "aviso".to_string());
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn truncado_de_texto_respeta_utf8() {
        let text = "áéíóú";
        let boundary = last_char_boundary(text, 3);
        assert!(text.is_char_boundary(boundary));
        assert_eq!(&text[..boundary], "á");
    }

    #[test]
    fn respeta_el_limite_de_paginas_y_emite_aviso() {
        let options = ExtractOptions {
            max_pages: 1,
            ..ExtractOptions::default()
        };
        let bytes = blank_pdf(3);
        let document = extract(Path::new("varias-paginas.pdf"), &bytes, &options)
            .expect("extrae páginas limitadas");
        assert_eq!(
            document.sections.len(),
            0,
            "las páginas vacías no dan chunks"
        );
        assert!(document
            .warnings
            .iter()
            .any(|warning| warning.contains("solo las primeras 1")));
    }
}
