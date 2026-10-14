//! Troceado de secciones extraídas de documentos (texto, CSV y, más adelante,
//! Office y PDF).
//!
//! A diferencia del chunker de Markdown, aquí no hay bloques de código ni
//! encabezados que interpretar: cada sección ya viene delimitada por el
//! extractor, con su encabezado y su ubicación. Solo hay que agrupar líneas
//! hasta el tamaño objetivo, cortando preferentemente en líneas en blanco.

use crate::chunk::{Chunk, ChunkMetadata, ChunkingOptions};

/// Sección de un documento extraído, lista para trocear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSection {
    /// Encabezado o contexto de la sección, si se conoce.
    pub heading: Option<String>,
    /// Etiqueta de ubicación en el documento original (p. ej. `pág. 3`).
    pub location: Option<String>,
    /// Texto de la sección.
    pub text: String,
}

impl DocumentSection {
    /// Crea una sección.
    pub fn new(heading: Option<String>, location: Option<String>, text: impl Into<String>) -> Self {
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

            chunks.push(Chunk {
                id,
                content_hash,
                content: text,
                metadata: ChunkMetadata {
                    source_file: source_file.clone(),
                    source_path: relative_path.to_string(),
                    heading: section.heading.clone(),
                    location: section.location.clone(),
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

    fn options(max_chars: usize, overlap_chars: usize) -> ChunkingOptions {
        ChunkingOptions {
            max_chars,
            overlap_chars,
        }
    }

    #[test]
    fn conserva_encabezado_y_ubicacion() {
        let sections = vec![DocumentSection::new(
            Some("nombre · edad".to_string()),
            Some("filas 1-2".to_string()),
            "nombre: Ana; edad: 30\nnombre: Luis; edad: 25",
        )];
        let chunks = chunk_sections("datos.csv", &sections, &options(800, 100));
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].metadata.source_path, "datos.csv");
        assert_eq!(chunks[0].metadata.source_file, "datos.csv");
        assert_eq!(chunks[0].metadata.heading.as_deref(), Some("nombre · edad"));
        assert_eq!(chunks[0].metadata.location.as_deref(), Some("filas 1-2"));
        assert!(chunks[0].content.contains("nombre: Ana"));
    }

    #[test]
    fn respeta_el_tamano_maximo() {
        let long = (0..60)
            .map(|index| format!("linea numero {index} con algo de texto"))
            .collect::<Vec<_>>()
            .join("\n");
        let sections = vec![DocumentSection::new(None, None, long)];
        let chunks = chunk_sections("notas.txt", &sections, &options(120, 20));
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(
                chunk.content.chars().count() <= 160,
                "fragmento demasiado largo: {}",
                chunk.content.chars().count()
            );
        }
    }

    #[test]
    fn secciones_vacias_no_producen_fragmentos() {
        let sections = vec![DocumentSection::new(None, None, "   \n  ")];
        assert!(chunk_sections("vacio.txt", &sections, &options(800, 100)).is_empty());
    }
}
