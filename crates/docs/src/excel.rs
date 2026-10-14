//! Extracción de hojas de cálculo (Excel `.xlsx`, `.xls`, `.xlsm`, `.xlsb`).
//!
//! Se apoya en `calamine`. Cada hoja produce una o varias secciones; la primera
//! línea de cada sección mapea las columnas a sus letras (`A=nombre, B=edad`) y
//! cada fila se emite con su número real (`2 | Ana | 30`). La ubicación cita
//! `hoja <nombre>, A2:D20`.

use std::path::Path;

use calamine::{open_workbook_auto, Data, Range, Reader};

use crate::{column_letter, DocKind, ExtractError, ExtractedDocument, Location, Result, Section};

/// Número de filas de datos por sección.
const ROWS_PER_SECTION: usize = 100;

/// Extrae todas las hojas del libro como secciones.
pub fn extract(path: &Path) -> Result<ExtractedDocument> {
    let mut workbook = open_workbook_auto(path).map_err(parse_error)?;
    let sheets = workbook.sheet_names().to_vec();
    let mut sections = Vec::new();

    for name in sheets {
        let range = workbook.worksheet_range(&name).map_err(parse_error)?;
        sections.extend(sheet_sections(&name, &range));
    }

    Ok(ExtractedDocument::new(DocKind::Excel, sections))
}

/// Convierte una hoja en secciones con coordenadas.
fn sheet_sections(name: &str, range: &Range<Data>) -> Vec<Section> {
    let (rows, cols) = range.get_size();
    if rows == 0 || cols == 0 {
        return Vec::new();
    }

    let (start_row, start_col) = range.start().unwrap_or((0, 0));
    let (start_row, start_col) = (start_row as usize, start_col as usize);
    let columns = format!(
        "{}:{}",
        column_letter(start_col),
        column_letter(start_col + cols - 1)
    );

    let mut row_iter = range.rows();
    // La primera fila del rango se usa como cabecera de columnas.
    let header = row_iter
        .next()
        .map(|row| column_mapping(row, start_col))
        .unwrap_or_default();

    let mut sections = Vec::new();
    let mut batch: Vec<String> = Vec::new();
    let mut first_row = start_row + 2;
    let mut last_row = first_row;

    for (index, row) in row_iter.enumerate() {
        // 1-based, contando la cabecera (la fila 0 del rango).
        let absolute = start_row + index + 2;
        let line = row_line(row);
        if line.is_empty() {
            continue;
        }
        if batch.is_empty() {
            first_row = absolute;
        }
        last_row = absolute;
        batch.push(format!("{absolute} | {line}"));
        if batch.len() >= ROWS_PER_SECTION {
            sections.push(make_section(
                name, &columns, &header, first_row, last_row, &batch,
            ));
            batch.clear();
        }
    }

    if !batch.is_empty() {
        sections.push(make_section(
            name, &columns, &header, first_row, last_row, &batch,
        ));
    }

    sections
}

/// Crea la sección de un lote de filas de una hoja.
fn make_section(
    sheet: &str,
    columns: &str,
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
        heading: Some(sheet.to_string()),
        location: Location::Rows {
            sheet: Some(sheet.to_string()),
            columns: Some(columns.to_string()),
            first_row,
            last_row,
            prefix_lines,
        },
        text,
    }
}

/// Mapea las columnas a sus letras a partir de una fila de cabecera.
fn column_mapping(row: &[Data], start_col: usize) -> String {
    let mut parts = Vec::new();
    for (index, cell) in row.iter().enumerate() {
        let value = cell_to_string(cell);
        if value.trim().is_empty() {
            continue;
        }
        parts.push(format!("{}={value}", column_letter(start_col + index)));
    }
    parts.join(", ")
}

/// Une los valores de una fila con ` | `, sin celdas vacías al final.
fn row_line(row: &[Data]) -> String {
    let mut values: Vec<String> = row.iter().map(cell_to_string).collect();
    while values.last().is_some_and(|value| value.trim().is_empty()) {
        values.pop();
    }
    values.join(" | ")
}

/// Convierte el valor de una celda en texto (vacío si no tiene contenido).
fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        other => other.to_string(),
    }
}

/// Traduce un error de calamine.
fn parse_error(error: calamine::Error) -> ExtractError {
    ExtractError::Parse(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn celdas_vacias_no_aportan_texto() {
        assert_eq!(cell_to_string(&Data::Empty), "");
        assert_eq!(cell_to_string(&Data::String("hola".to_string())), "hola");
        assert_eq!(cell_to_string(&Data::Int(7)), "7");
    }

    #[test]
    fn mapea_columnas_a_letras() {
        let row = vec![
            Data::String("nombre".to_string()),
            Data::String("edad".to_string()),
        ];
        assert_eq!(column_mapping(&row, 0), "A=nombre, B=edad");
        assert_eq!(column_mapping(&row, 2), "C=nombre, D=edad");
    }

    #[test]
    fn une_filas_sin_vacios_finales() {
        let row = vec![Data::Int(1), Data::Empty, Data::Int(3), Data::Empty];
        assert_eq!(row_line(&row), "1 |  | 3");
    }

    #[test]
    fn un_archivo_inexistente_da_error() {
        assert!(extract(Path::new("/tmp/inexistente.xlsx")).is_err());
    }
}
