//! Extracción de valores separados por comas (CSV).
//!
//! Cada fila se convierte en una línea `columna: valor` para que el contenido
//! sea buscable, y las filas se agrupan en secciones de tamaño acotado. La
//! cabecera se usa como encabezado de la sección.

use crate::{DocKind, ExtractError, ExtractedDocument, Location, Result, Section};

/// Número de filas de datos por sección.
const ROWS_PER_SECTION: usize = 100;

/// Nombre que se usa cuando una columna no tiene cabecera.
const FALLBACK_COLUMN: &str = "columna";

/// Extrae un CSV como una o varias secciones, una por lote de filas.
pub fn extract(bytes: &[u8]) -> Result<ExtractedDocument> {
    let mut reader = ::csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(bytes);

    let headers: Vec<String> = reader
        .headers()
        .map_err(parse_error)?
        .iter()
        .map(|header| header.trim().to_string())
        .collect();

    let mut sections = Vec::new();
    let mut batch: Vec<String> = Vec::new();
    let mut batch_start = 1usize;
    let mut row_number = 0usize;

    for record in reader.records() {
        let record = record.map_err(parse_error)?;
        row_number += 1;
        batch.push(format_row(&headers, record.iter()));
        if batch.len() >= ROWS_PER_SECTION {
            sections.push(make_section(&headers, batch_start, row_number, &batch));
            batch.clear();
            batch_start = row_number + 1;
        }
    }

    if !batch.is_empty() {
        sections.push(make_section(&headers, batch_start, row_number, &batch));
    }

    Ok(ExtractedDocument {
        kind: DocKind::Csv,
        sections,
    })
}

/// Convierte una fila en `columna: valor; …`.
fn format_row<'a>(headers: &[String], values: impl Iterator<Item = &'a str>) -> String {
    let mut line = String::new();
    for (index, value) in values.enumerate() {
        if !line.is_empty() {
            line.push_str("; ");
        }
        let column = headers
            .get(index)
            .map(String::as_str)
            .filter(|name| !name.is_empty())
            .unwrap_or(FALLBACK_COLUMN);
        line.push_str(column);
        line.push_str(": ");
        line.push_str(value.trim());
    }
    line
}

/// Crea la sección de un lote de filas.
fn make_section(headers: &[String], start: usize, end: usize, rows: &[String]) -> Section {
    let heading = headers
        .iter()
        .filter(|header| !header.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(" · ");

    Section {
        heading: (!heading.is_empty()).then_some(heading),
        location: Location::Rows { start, end },
        text: rows.join("\n"),
    }
}

/// Traduce un error del lector CSV.
fn parse_error(error: ::csv::Error) -> ExtractError {
    ExtractError::Parse(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convierte_cabecera_y_filas() {
        let csv = "nombre,edad\nAna,30\nLuis,25\n";
        let document = extract(csv.as_bytes()).expect("extrae");
        assert_eq!(document.kind, DocKind::Csv);
        assert_eq!(document.sections.len(), 1);

        let section = &document.sections[0];
        assert_eq!(section.heading.as_deref(), Some("nombre · edad"));
        assert_eq!(section.location, Location::Rows { start: 1, end: 2 });
        assert!(section.text.contains("nombre: Ana; edad: 30"));
        assert!(section.text.contains("nombre: Luis; edad: 25"));
    }

    #[test]
    fn agrupa_por_lotes_de_filas() {
        let mut csv = String::from("id,valor\n");
        for index in 0..250 {
            csv.push_str(&format!("{index},{}\n", index * 2));
        }
        let document = extract(csv.as_bytes()).expect("extrae");
        assert_eq!(document.sections.len(), 3);
        assert_eq!(
            document.sections[0].location,
            Location::Rows { start: 1, end: 100 }
        );
        assert_eq!(
            document.sections[2].location,
            Location::Rows {
                start: 201,
                end: 250
            }
        );
    }

    #[test]
    fn tolera_filas_con_menos_columnas() {
        let csv = "a,b,c\n1,2\n";
        let document = extract(csv.as_bytes()).expect("extrae");
        assert!(document.sections[0].text.contains("a: 1; b: 2"));
    }

    #[test]
    fn cabecera_sin_filas_no_produce_secciones() {
        let document = extract(b"solo,cabecera\n").expect("extrae");
        assert!(document.sections.is_empty());
    }
}
