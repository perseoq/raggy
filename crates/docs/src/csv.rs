//! Extracción de valores separados por comas (CSV).
//!
//! Como un CSV no tiene hojas, se cita como «hoja 1». La primera línea de cada
//! sección mapea las columnas a sus letras (`A=nombre, B=edad`) y cada fila se
//! emite con su número real (la cabecera es la fila 1). La ubicación cita
//! `hoja 1, A2:C100`.

use crate::{column_letter, DocKind, ExtractError, ExtractedDocument, Location, Result, Section};

/// Número de filas de datos por sección.
const ROWS_PER_SECTION: usize = 100;

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

    let columns = (!headers.is_empty())
        .then(|| format!("{}:{}", column_letter(0), column_letter(headers.len() - 1)));
    let header_line = headers
        .iter()
        .enumerate()
        .filter(|(_, name)| !name.is_empty())
        .map(|(index, name)| format!("{}={name}", column_letter(index)))
        .collect::<Vec<_>>()
        .join(", ");

    let mut sections = Vec::new();
    let mut batch: Vec<String> = Vec::new();
    let mut row_number = 1usize; // la cabecera es la fila 1
    let mut first_row = 2usize;
    let mut last_row = 2usize;

    for record in reader.records() {
        let record = record.map_err(parse_error)?;
        row_number += 1;
        let line = row_line(record.iter());
        if line.is_empty() {
            continue;
        }
        if batch.is_empty() {
            first_row = row_number;
        }
        last_row = row_number;
        batch.push(format!("{row_number} | {line}"));
        if batch.len() >= ROWS_PER_SECTION {
            sections.push(make_section(
                columns.clone(),
                &header_line,
                first_row,
                last_row,
                &batch,
            ));
            batch.clear();
        }
    }

    if !batch.is_empty() {
        sections.push(make_section(
            columns,
            &header_line,
            first_row,
            last_row,
            &batch,
        ));
    }

    Ok(ExtractedDocument::new(DocKind::Csv, sections))
}

/// Une los valores de una fila con ` | `, sin celdas vacías al final.
fn row_line<'a>(values: impl Iterator<Item = &'a str>) -> String {
    let mut values: Vec<&str> = values.map(str::trim).collect();
    while values.last().is_some_and(|value| value.is_empty()) {
        values.pop();
    }
    values.join(" | ")
}

/// Crea la sección de un lote de filas.
fn make_section(
    columns: Option<String>,
    header: &str,
    first_row: usize,
    last_row: usize,
    rows: &[String],
) -> Section {
    let prefix_lines = usize::from(!header.is_empty());
    let mut text = String::new();
    if !header.is_empty() {
        text.push_str("Columnas: ");
        text.push_str(header);
        text.push('\n');
    }
    text.push_str(&rows.join("\n"));

    Section {
        heading: None,
        location: Location::Rows {
            sheet: None,
            columns,
            first_row,
            last_row,
            prefix_lines,
        },
        text,
    }
}

/// Traduce un error del lector CSV.
fn parse_error(error: ::csv::Error) -> ExtractError {
    ExtractError::Parse(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn csv_section(bytes: &[u8]) -> Section {
        let document = extract(bytes).expect("extrae");
        assert_eq!(document.kind, DocKind::Csv);
        document.sections.into_iter().next().expect("sección")
    }

    #[test]
    fn numera_filas_y_mapea_columnas() {
        let section = csv_section(b"nombre,edad\nAna,30\nLuis,25\n");
        assert!(section.text.contains("Columnas: A=nombre, B=edad"));
        assert!(section.text.contains("2 | Ana | 30"));
        assert!(section.text.contains("3 | Luis | 25"));
        assert_eq!(
            section.location,
            Location::Rows {
                sheet: None,
                columns: Some("A:B".to_string()),
                first_row: 2,
                last_row: 3,
                prefix_lines: 1,
            }
        );
    }

    #[test]
    fn agrupa_por_lotes_de_filas() {
        let mut csv = String::from("id,valor\n");
        for index in 0..250 {
            csv.push_str(&format!("{index},{}\n", index * 2));
        }
        let document = extract(csv.as_bytes()).expect("extrae");
        assert_eq!(document.sections.len(), 3);
        match &document.sections[2].location {
            Location::Rows {
                first_row,
                last_row,
                ..
            } => {
                assert_eq!((*first_row, *last_row), (202, 251));
            }
            other => panic!("ubicación inesperada: {other:?}"),
        }
    }

    #[test]
    fn tolera_filas_con_menos_columnas() {
        let section = csv_section(b"a,b,c\n1,2\n");
        assert!(section.text.contains("2 | 1 | 2"));
    }

    #[test]
    fn cabecera_sin_filas_no_produce_secciones() {
        let document = extract(b"solo,cabecera\n").expect("extrae");
        assert!(document.sections.is_empty());
    }
}
