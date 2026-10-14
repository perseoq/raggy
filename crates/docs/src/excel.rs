//! Extracción de hojas de cálculo (Excel `.xlsx`, `.xls`, `.xlsm`, `.xlsb`).
//!
//! Se apoya en `calamine` para leer los formatos moderno y antiguo. Cada hoja
//! produce una o varias secciones; cada fila se convierte en una línea con las
//! celdas separadas por ` | `, y la ubicación cita `hoja <nombre>, filas N-M`.

use std::path::Path;

use calamine::{open_workbook_auto, Data, Reader};

use crate::{DocKind, ExtractError, ExtractedDocument, Location, Result, Section};

/// Número de filas por sección.
const ROWS_PER_SECTION: usize = 100;

/// Extrae todas las hojas del libro como secciones.
pub fn extract(path: &Path) -> Result<ExtractedDocument> {
    let mut workbook = open_workbook_auto(path).map_err(parse_error)?;
    let sheets = workbook.sheet_names().to_vec();
    let mut sections = Vec::new();

    for name in sheets {
        let range = workbook.worksheet_range(&name).map_err(parse_error)?;
        let mut batch: Vec<String> = Vec::new();
        let mut start = 1usize;
        let mut row_number = 0usize;

        for row in range.rows() {
            let line = row
                .iter()
                .map(cell_to_string)
                .collect::<Vec<_>>()
                .join(" | ");
            if line.trim().is_empty() {
                continue;
            }

            row_number += 1;
            batch.push(line);
            if batch.len() >= ROWS_PER_SECTION {
                sections.push(make_section(&name, start, row_number, &batch));
                batch.clear();
                start = row_number + 1;
            }
        }

        if !batch.is_empty() {
            sections.push(make_section(&name, start, row_number, &batch));
        }
    }

    Ok(ExtractedDocument {
        kind: DocKind::Excel,
        sections,
    })
}

/// Crea la sección de un lote de filas de una hoja.
fn make_section(sheet: &str, start: usize, end: usize, rows: &[String]) -> Section {
    Section {
        heading: Some(sheet.to_string()),
        location: Location::Sheet {
            name: sheet.to_string(),
            start,
            end,
        },
        text: rows.join("\n"),
    }
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
    fn un_archivo_inexistente_da_error() {
        assert!(extract(Path::new("/tmp/inexistente.xlsx")).is_err());
    }
}
