//! Troceado de secciones extraídas de documentos (texto, CSV, Office y PDF).
//!
//! A diferencia del chunker de Markdown, aquí no hay bloques de código ni
//! encabezados que interpretar: cada sección ya viene delimitada por el
//! extractor, con su encabezado y su ubicación. Solo hay que agrupar líneas
//! hasta el tamaño objetivo, cortando preferentemente en líneas en blanco, y
//! calcular la **ubicación de cada fragmento** a partir de la de la sección.

use crate::chunk::{Chunk, ChunkMetadata, ChunkingOptions};

/// Ubicación de una sección (y de cada uno de sus fragmentos) en el documento
/// original.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionLocation {
    /// Líneas del texto extraído; `start` es la primera línea de la sección
    /// (1-based).
    Lines {
        /// Primera línea de la sección.
        start: usize,
    },
    /// Filas de una hoja de cálculo o de un CSV.
    Rows {
        /// Nombre de la hoja; `None` para CSV (se cita como «hoja 1»).
        sheet: Option<String>,
        /// Rango de columnas en letras, p. ej. `A:D`.
        columns: Option<String>,
        /// Número de la primera fila de datos de la sección (1-based).
        first_row: usize,
        /// Líneas iniciales de la sección que no son filas (p. ej. cabecera).
        prefix_lines: usize,
    },
    /// Página de un PDF.
    Page(u32),
    /// Diapositiva de una presentación.
    Slide(u32),
}

impl SectionLocation {
    /// Etiqueta legible del fragmento que cubre las líneas `[start, end)` de la
    /// sección (índices 0-based dentro del texto de la sección).
    pub fn label_for_range(&self, start: usize, end: usize) -> String {
        let end = end.max(start + 1);
        match self {
            SectionLocation::Lines { start: base } => {
                format!("líneas {}-{}", base + start, base + end - 1)
            }
            SectionLocation::Rows {
                sheet,
                columns,
                first_row,
                prefix_lines,
            } => {
                let sheet = sheet.clone().unwrap_or_else(|| "1".to_string());
                let data_start = start.max(*prefix_lines);
                if end <= data_start {
                    return format!("hoja {sheet}");
                }
                let row_first = first_row + (data_start - prefix_lines);
                let row_last = first_row + (end - 1 - prefix_lines);
                format!("hoja {sheet}, {}", cell_range(columns, row_first, row_last))
            }
            SectionLocation::Page(number) => format!("página {number}"),
            SectionLocation::Slide(number) => format!("diapositiva {number}"),
        }
    }
}

/// Cita de un rango de celdas, p. ej. `A2:D20`, o `fila 7` cuando es una sola.
fn cell_range(columns: &Option<String>, first_row: usize, last_row: usize) -> String {
    if first_row == last_row {
        return format!("fila {first_row}");
    }
    match columns
        .as_deref()
        .and_then(|columns| columns.split_once(':'))
    {
        Some((first, last)) => format!("{first}{first_row}:{last}{last_row}"),
        None => format!("filas {first_row}-{last_row}"),
    }
}

/// Sección de un documento extraído, lista para trocear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSection {
    /// Encabezado o contexto de la sección, si se conoce.
    pub heading: Option<String>,
    /// Ubicación de la sección en el documento original.
    pub location: Option<SectionLocation>,
    /// Texto de la sección.
    pub text: String,
}

impl DocumentSection {
    /// Crea una sección.
    pub fn new(
        heading: Option<String>,
        location: Option<SectionLocation>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            heading,
            location,
            text: text.into(),
        }
    }
}

/// Trocea las secciones de un documento en fragmentos listos para indexar.
///
/// `relative_path` es la ruta mostrada en las citas; los números de línea son
/// los del texto extraído (orientativos para formatos que no son Markdown).
pub fn chunk_sections(
    relative_path: &str,
    sections: &[DocumentSection],
    options: &ChunkingOptions,
) -> Vec<Chunk> {
    let source_file = file_name(relative_path);
    let mut chunks = Vec::new();
    let mut line_offset = 0usize;

    for section in sections {
        let lines: Vec<&str> = section.text.split('\n').collect();
        for (start, end) in split_ranges(&lines, options) {
            let text = lines[start..end].join("\n");
            if text.trim().is_empty() {
                continue;
            }

            let content_hash = blake3::hash(text.as_bytes()).to_hex().to_string();
            let start_line = line_offset + start + 1;
            let end_line = line_offset + end;
            let id = blake3::hash(
                format!("{relative_path}:{start_line}:{end_line}:{content_hash}").as_bytes(),
            )
            .to_hex()
            .to_string();

            let location = section
                .location
                .as_ref()
                .map(|location| location.label_for_range(start, end));

            chunks.push(Chunk {
                id,
                content_hash,
                content: text,
                metadata: ChunkMetadata {
                    source_file: source_file.clone(),
                    source_path: relative_path.to_string(),
                    heading: section.heading.clone(),
                    location,
                    start_line,
                    end_line,
                },
            });
        }

        line_offset += lines.len();
    }

    chunks
}

/// Reparte las líneas en rangos `[inicio, fin)` que respetan el tamaño y el
/// solapamiento. Se prefiere cortar tras una línea en blanco.
fn split_ranges(lines: &[&str], options: &ChunkingOptions) -> Vec<(usize, usize)> {
    let max_chars = options.max_chars.max(1);
    let overlap = options.overlap_chars.min(max_chars.saturating_sub(1));
    let mut result = Vec::new();
    let mut cursor = 0usize;

    while cursor < lines.len() {
        let mut scan = cursor;
        let mut chars = 0usize;
        let mut preferred: Option<usize> = None;
        let mut fallback: Option<usize> = None;

        while scan < lines.len() {
            let line_len = lines[scan].chars().count() + 1;
            if scan > cursor && chars + line_len > max_chars {
                break;
            }
            chars += line_len;
            scan += 1;

            if scan < lines.len() {
                if lines[scan - 1].trim().is_empty() {
                    preferred = Some(scan);
                } else {
                    fallback = Some(scan);
                }
            }
        }

        let mut chunk_end = if scan >= lines.len() {
            lines.len()
        } else {
            preferred.or(fallback).unwrap_or(scan)
        };
        chunk_end = chunk_end.clamp(cursor + 1, lines.len());

        result.push((cursor, chunk_end));
        if chunk_end >= lines.len() {
            break;
        }

        // Solapamiento: se reincorporan líneas finales hasta el límite.
        let mut overlap_start = chunk_end;
        let mut used = 0usize;
        while overlap_start > cursor {
            let previous = overlap_start - 1;
            let line_len = lines[previous].chars().count() + 1;
            if used + line_len > overlap {
                break;
            }
            used += line_len;
            overlap_start -= 1;
        }

        cursor = if overlap_start > cursor && overlap_start < chunk_end {
            overlap_start
        } else {
            chunk_end
        };
    }

    result
}

/// Nombre de archivo a partir de una ruta relativa.
fn file_name(relative_path: &str) -> String {
    relative_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(relative_path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Citation;

    fn options(max_chars: usize, overlap_chars: usize) -> ChunkingOptions {
        ChunkingOptions {
            max_chars,
            overlap_chars,
        }
    }

    #[test]
    fn etiqueta_por_rango_de_filas() {
        let location = SectionLocation::Rows {
            sheet: Some("Ventas".to_string()),
            columns: Some("A:D".to_string()),
            first_row: 2,
            prefix_lines: 1,
        };
        // La primera línea es la cabecera; las siguientes son filas 2, 3, ...
        assert_eq!(location.label_for_range(1, 4), "hoja Ventas, A2:D4");
        assert_eq!(location.label_for_range(0, 1), "hoja Ventas");
    }

    #[test]
    fn etiqueta_por_rango_de_lineas() {
        let location = SectionLocation::Lines { start: 10 };
        assert_eq!(location.label_for_range(0, 3), "líneas 10-12");
    }

    #[test]
    fn etiquetas_constantes() {
        assert_eq!(SectionLocation::Page(3).label_for_range(0, 1), "página 3");
        assert_eq!(
            SectionLocation::Slide(2).label_for_range(0, 5),
            "diapositiva 2"
        );
    }

    #[test]
    fn conserva_encabezado_y_ubicacion() {
        let sections = vec![DocumentSection::new(
            Some("Ventas".to_string()),
            Some(SectionLocation::Rows {
                sheet: Some("Ventas".to_string()),
                columns: Some("A:B".to_string()),
                first_row: 1,
                prefix_lines: 0,
            }),
            "nombre: Ana; edad: 30\nnombre: Luis; edad: 25",
        )];
        let chunks = chunk_sections("datos.csv", &sections, &options(800, 100));
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].metadata.source_path, "datos.csv");
        assert_eq!(chunks[0].metadata.source_file, "datos.csv");
        assert_eq!(chunks[0].metadata.heading.as_deref(), Some("Ventas"));
        assert_eq!(
            chunks[0].metadata.location.as_deref(),
            Some("hoja Ventas, A1:B2")
        );
        assert!(chunks[0].content.contains("nombre: Ana"));
    }

    #[test]
    fn respeta_el_tamano_maximo() {
        let long = (0..60)
            .map(|index| format!("linea numero {index} con algo de texto"))
            .collect::<Vec<_>>()
            .join("\n");
        let sections = vec![DocumentSection::new(
            None,
            Some(SectionLocation::Lines { start: 1 }),
            long,
        )];
        let chunks = chunk_sections("notas.txt", &sections, &options(120, 20));
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(
                chunk.content.chars().count() <= 160,
                "fragmento demasiado largo: {}",
                chunk.content.chars().count()
            );
            assert!(chunk.metadata.location.is_some());
        }
    }

    #[test]
    fn secciones_vacias_no_producen_fragmentos() {
        let sections = vec![DocumentSection::new(None, None, "   \n  ")];
        assert!(chunk_sections("vacio.txt", &sections, &options(800, 100)).is_empty());
    }

    #[test]
    fn la_cita_identifica_el_formato_y_su_ubicacion() {
        // Cada formato se cita con la unidad que le es propia: página, diapositiva,
        // fila (una sola, porque cada fila es un fragmento) o rango de líneas.
        let sections = vec![
            DocumentSection::new(None, Some(SectionLocation::Page(7)), "texto de la página"),
            DocumentSection::new(
                None,
                Some(SectionLocation::Slide(3)),
                "texto de la diapositiva",
            ),
            DocumentSection::new(
                None,
                Some(SectionLocation::Rows {
                    sheet: None,
                    columns: Some("A:M".to_string()),
                    first_row: 2669,
                    prefix_lines: 0,
                }),
                "2669 | C-2026-02668 | Pendiente",
            ),
            DocumentSection::new(
                None,
                Some(SectionLocation::Lines { start: 10 }),
                "linea diez",
            ),
        ];
        let chunks = chunk_sections("mezcla.txt", &sections, &options(800, 100));

        let etiquetas: Vec<String> = chunks
            .iter()
            .map(|chunk| Citation::from_chunk(chunk, 0.0).label())
            .collect();
        assert_eq!(
            etiquetas,
            vec![
                "mezcla.txt (página 7)",
                "mezcla.txt (diapositiva 3)",
                "mezcla.txt (hoja 1, fila 2669)",
                "mezcla.txt (líneas 10-10)",
            ]
        );
    }

    #[test]
    fn markdown_y_texto_sin_ubicacion_se_citan_por_lineas() {
        // El chunker de Markdown deja la ubicación a `None`: la cita cae al rango
        // de líneas del fragmento, que es la unidad en la que se ancla la respuesta.
        let chunk = Chunk {
            id: "x".to_string(),
            content_hash: "x".to_string(),
            content: "# Título\n\nContenido".to_string(),
            metadata: ChunkMetadata {
                source_file: "guia.md".to_string(),
                source_path: "docs/guia.md".to_string(),
                heading: Some("Título".to_string()),
                location: None,
                start_line: 10,
                end_line: 14,
            },
        };
        assert_eq!(
            Citation::from_chunk(&chunk, 0.0).label(),
            "docs/guia.md (líneas 10-14)"
        );
    }
}
