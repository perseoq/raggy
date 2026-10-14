//! Chunker semántico de Markdown (HU-09).
//!
//! Estrategia:
//!
//! 1. Se localizan los encabezados reales con `pulldown-cmark` (no los `#`
//!    que aparecen dentro de bloques de código) y se dividen las secciones,
//!    anotando el camino de encabezados de cada una.
//! 2. Dentro de cada sección se agrupan líneas hasta el tamaño objetivo,
//!    prefiriendo cortes en líneas en blanco.
//! 3. Un bloque de código *nunca* se parte: los cortes solo se permiten fuera
//!    de una región cercada con ``` o ~~~.
//! 4. Cada fragmento incluye metadatos (archivo, ruta, encabezado, líneas).

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::chunk::{Chunk, ChunkMetadata, ChunkSource, Chunker, ChunkingOptions};

/// Chunker semántico basado en la estructura de Markdown.
#[derive(Debug, Default, Clone, Copy)]
pub struct MarkdownChunker;

impl MarkdownChunker {
    /// Crea una instancia del chunker.
    pub fn new() -> Self {
        Self
    }
}

impl Chunker for MarkdownChunker {
    fn name(&self) -> &'static str {
        "markdown-estructurado"
    }

    fn chunk(&self, source: &ChunkSource<'_>, options: &ChunkingOptions) -> Vec<Chunk> {
        chunk_markdown(source, options)
    }
}

/// Divide un documento Markdown en fragmentos.
pub fn chunk_markdown(source: &ChunkSource<'_>, options: &ChunkingOptions) -> Vec<Chunk> {
    let content = source.content;
    if content.trim().is_empty() {
        return Vec::new();
    }

    let lines: Vec<&str> = content.split('\n').collect();
    if lines.is_empty() {
        return Vec::new();
    }

    let lens: Vec<usize> = lines.iter().map(|line| line.chars().count()).collect();
    let (is_blank, in_code) = line_flags(&lines);
    let starts = line_starts(content);

    let max_chars = options.max_chars.max(1);
    let overlap = options.overlap_chars.min(max_chars.saturating_sub(1));

    let mut chunks = Vec::new();
    for section in sections(content, &starts, lines.len()) {
        let ranges = split_lines(
            &lens,
            &is_blank,
            &in_code,
            section.start,
            section.end,
            max_chars,
            overlap,
        );
        for (start, end) in ranges {
            let (start, end) = trim_blank(&lines, start, end);
            if start >= end {
                continue;
            }
            let text = lines[start..end].join("\n");
            if text.trim().is_empty() {
                continue;
            }

            let content_hash = blake3::hash(text.as_bytes()).to_hex().to_string();
            let id = blake3::hash(
                format!(
                    "{}:{}:{}:{}",
                    source.relative_path,
                    start + 1,
                    end,
                    content_hash
                )
                .as_bytes(),
            )
            .to_hex()
            .to_string();

            chunks.push(Chunk {
                id,
                content_hash,
                content: text,
                metadata: ChunkMetadata {
                    source_file: file_name(source.relative_path),
                    source_path: source.relative_path.to_string(),
                    heading: section.heading.clone(),
                    location: None,
                    start_line: start + 1,
                    end_line: end,
                },
            });
        }
    }

    chunks
}

/// Sección del documento delimitada por encabezados.
struct Section {
    start: usize,
    end: usize,
    heading: Option<String>,
}

/// Construye las secciones a partir de los encabezados detectados.
fn sections(content: &str, starts: &[usize], total_lines: usize) -> Vec<Section> {
    let headings = heading_positions(content, starts);
    let mut result = Vec::new();

    let mut prev_line = 0usize;
    let mut prev_heading: Option<String> = None;

    for (line, path) in headings {
        let line = line.min(total_lines);
        if line > prev_line {
            result.push(Section {
                start: prev_line,
                end: line,
                heading: prev_heading.clone(),
            });
        }
        prev_line = line;
        prev_heading = Some(path.join(" > "));
    }

    if prev_line < total_lines {
        result.push(Section {
            start: prev_line,
            end: total_lines,
            heading: prev_heading,
        });
    }

    result
}

/// Devuelve, para cada encabezado, la línea donde empieza y su camino completo.
fn heading_positions(content: &str, starts: &[usize]) -> Vec<(usize, Vec<String>)> {
    let mut result = Vec::new();
    let mut stack: Vec<(u8, String)> = Vec::new();
    let mut current: Option<(u8, String)> = None;

    for (event, range) in Parser::new_ext(content, Options::empty()).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                current = Some((heading_level(level), String::new()));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, text)) = current.take() {
                    let text = text.trim().to_string();
                    stack.retain(|(l, _)| *l < level);
                    stack.push((level, text));
                    let path = stack.iter().map(|(_, t)| t.clone()).collect();
                    result.push((line_of(starts, range.start), path));
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, buffer)) = current.as_mut() {
                    buffer.push_str(&text);
                }
            }
            _ => {}
        }
    }

    result
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Offset de inicio de cada línea (índice 0 = inicio del documento).
fn line_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (index, byte) in content.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(index + 1);
        }
    }
    starts
}

/// Número de línea (0-based) que contiene el offset dado.
fn line_of(starts: &[usize], byte: usize) -> usize {
    match starts.binary_search(&byte) {
        Ok(index) => index,
        Err(index) => index.saturating_sub(1),
    }
}

/// Marca las líneas en blanco y las que están dentro de un bloque cercado.
fn line_flags(lines: &[&str]) -> (Vec<bool>, Vec<bool>) {
    let mut in_code = vec![false; lines.len()];
    let mut is_blank = vec![false; lines.len()];
    let mut fence: Option<(char, usize)> = None;

    for (index, raw) in lines.iter().enumerate() {
        let trimmed = raw.trim_start();
        is_blank[index] = raw.trim().is_empty();

        if let Some((open_char, open_len)) = fence {
            in_code[index] = true;
            if let Some((close_char, close_len)) = fence_marker(trimmed) {
                if close_char == open_char && close_len >= open_len {
                    fence = None;
                }
            }
        } else if let Some((open_char, open_len)) = fence_marker(trimmed) {
            in_code[index] = true;
            fence = Some((open_char, open_len));
        }
    }

    (is_blank, in_code)
}

/// Detecta el inicio de un cerco de código (``` o ~~~).
fn fence_marker(trimmed: &str) -> Option<(char, usize)> {
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|&c| c == marker).count();
    (count >= 3).then_some((marker, count))
}

/// Reparte el rango `[start, end)` en fragmentos que respetan el tamaño y los
/// bloques de código. Devuelve rangos de líneas `[inicio, fin)`.
fn split_lines(
    lens: &[usize],
    is_blank: &[bool],
    in_code: &[bool],
    start: usize,
    end: usize,
    max_chars: usize,
    overlap_chars: usize,
) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    let mut cursor = start;

    while cursor < end {
        let mut scan = cursor;
        let mut chars = 0usize;
        let mut preferred: Option<usize> = None;
        let mut fallback: Option<usize> = None;

        while scan < end {
            let line_len = lens[scan] + 1;
            if scan > cursor && chars + line_len > max_chars {
                break;
            }
            chars += line_len;
            scan += 1;

            if scan < end && !in_code[scan] {
                if is_blank[scan - 1] {
                    // Se prefiere el ÚLTIMO corte en línea en blanco que quepa,
                    // para aprovechar el límite en lugar de partir cada párrafo.
                    preferred = Some(scan);
                } else {
                    fallback = Some(scan);
                }
            }
        }

        // Si todo el rango cabe en el límite, se toma entero; si se cortó por
        // tamaño, se usa el último corte válido que quepa.
        let mut chunk_end = if scan >= end {
            end
        } else {
            preferred.or(fallback).unwrap_or(scan)
        };
        // Nunca cortar dentro de un bloque de código.
        while chunk_end < end && in_code[chunk_end] {
            chunk_end += 1;
        }
        if chunk_end <= cursor {
            chunk_end = (cursor + 1).min(end);
        }

        result.push((cursor, chunk_end));
        if chunk_end >= end {
            break;
        }

        // Solapamiento: se reincorporan líneas finales que no sean código.
        let mut overlap_start = chunk_end;
        let mut overlap_chars_used = 0usize;
        while overlap_start > cursor {
            let previous = overlap_start - 1;
            if in_code[previous] {
                break;
            }
            let line_len = lens[previous] + 1;
            if overlap_chars_used + line_len > overlap_chars {
                break;
            }
            overlap_chars_used += line_len;
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

/// Recorta líneas en blanco al principio y al final de un rango.
fn trim_blank(lines: &[&str], start: usize, end: usize) -> (usize, usize) {
    let mut start = start;
    let mut end = end;
    while start < end && lines[start].trim().is_empty() {
        start += 1;
    }
    while end > start && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    (start, end)
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
    use crate::chunk::ChunkSource;

    fn source(content: &str) -> ChunkSource<'_> {
        ChunkSource {
            relative_path: "docs/guia.md",
            content,
        }
    }

    fn small_options() -> ChunkingOptions {
        ChunkingOptions {
            max_chars: 120,
            overlap_chars: 20,
        }
    }

    #[test]
    fn no_parte_bloques_de_codigo() {
        let code = (0..40)
            .map(|i| format!("    let valor_{i} = {i};"))
            .collect::<Vec<_>>()
            .join("\n");
        let content =
            format!("# Ejemplo\n\nTexto previo.\n\n```rust\n{code}\n```\n\nTexto posterior.\n");

        let chunks = chunk_markdown(&source(&content), &small_options());
        assert!(chunks.len() > 1, "debería dividirse en varios fragmentos");

        for chunk in &chunks {
            let fences = chunk
                .content
                .lines()
                .filter(|line| fence_marker(line.trim_start()).is_some())
                .count();
            assert_eq!(
                fences % 2,
                0,
                "fragmento con cerco impar:\n{}",
                chunk.content
            );
        }

        let con_bloque = chunks
            .iter()
            .find(|chunk| chunk.content.contains("let valor_0"))
            .expect("algún fragmento contiene el código");
        assert!(con_bloque.content.contains("```"));
        assert!(con_bloque.content.contains("let valor_39"));
    }

    #[test]
    fn anota_encabezados_y_lineas() {
        let content = "# Documento\n\nIntro.\n\n## Detalle\n\nContenido del detalle.\n";
        let chunks = chunk_markdown(&source(content), &ChunkingOptions::default());

        let headings: Vec<_> = chunks.iter().map(|c| c.metadata.heading.clone()).collect();
        assert!(headings.iter().any(|h| h.as_deref() == Some("Documento")));
        assert!(headings
            .iter()
            .any(|h| h.as_deref() == Some("Documento > Detalle")));

        let detalle = chunks
            .iter()
            .find(|c| c.content.contains("Contenido del detalle"))
            .expect("existe el fragmento de la subsección");
        assert_eq!(detalle.metadata.source_file, "guia.md");
        assert_eq!(detalle.metadata.source_path, "docs/guia.md");
        assert_eq!(
            detalle.metadata.heading.as_deref(),
            Some("Documento > Detalle")
        );
        assert!(detalle.metadata.start_line <= detalle.metadata.end_line);
    }

    #[test]
    fn ignora_encabezados_dentro_de_codigo() {
        let content = "# Título\n\n```\n# no es encabezado\n```\n";
        let chunks = chunk_markdown(&source(content), &ChunkingOptions::default());
        assert!(chunks
            .iter()
            .all(|c| c.metadata.heading.as_deref() == Some("Título")));
    }

    #[test]
    fn agrupa_parrafos_hasta_el_limite() {
        let content = "# Doc\n\n\
            Párrafo uno con bastante texto para ocupar espacio y no ser trivial.\n\n\
            Párrafo dos con bastante texto para ocupar espacio y no ser trivial.\n\n\
            Párrafo tres con bastante texto para ocupar espacio y no ser trivial.\n";
        let options = ChunkingOptions {
            max_chars: 400,
            overlap_chars: 0,
        };
        let chunks = chunk_markdown(&source(content), &options);
        assert_eq!(
            chunks.len(),
            1,
            "los párrafos deben agruparse en un solo fragmento, no uno por párrafo"
        );
    }

    #[test]
    fn no_genera_chunks_vacios() {
        let content = "\n\n# A\n\n\n\ntexto\n\n\n# B\n\nmás texto\n\n\n";
        let chunks = chunk_markdown(&source(content), &ChunkingOptions::default());
        assert!(chunks.iter().all(|chunk| !chunk.content.trim().is_empty()));
    }

    #[test]
    fn es_determinista() {
        let content = "\n\n# A\n\n\n\ntexto\n\n\n# B\n\nmás texto\n\n\n";
        let options = ChunkingOptions::default();
        let first = chunk_markdown(&source(content), &options);
        let second = chunk_markdown(&source(content), &options);

        assert_eq!(
            first.iter().map(|c| &c.id).collect::<Vec<_>>(),
            second.iter().map(|c| &c.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn documento_vacio_no_produce_fragmentos() {
        assert!(chunk_markdown(&source("   \n\n"), &ChunkingOptions::default()).is_empty());
    }
}
