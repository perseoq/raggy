//! Diagnóstico de las herramientas externas de la ingesta (OCR y conversión).

use raggy_docs::tools::{find_libreoffice, install_hint, invalidate_cache, ExternalTool, Tools};

/// Informe legible del estado de Tesseract, Poppler y LibreOffice, con las
/// pistas de instalación cuando falten. Fuerza una detección nueva para reflejar
/// una instalación reciente (el botón «Comprobar herramientas»).
pub fn tools_report(languages: &[String], timeout_secs: u64) -> String {
    invalidate_cache();
    let tools = Tools::detect_with_timeout(timeout_secs);
    let mut lines = Vec::new();

    match &tools.tesseract {
        Some(program) => {
            let list = if tools.languages.is_empty() {
                "(no se pudieron listar)".to_string()
            } else {
                tools.languages.join(", ")
            };
            lines.push(format!(
                "Tesseract: {} — idiomas: {list}",
                program.display()
            ));
            if !tools.languages_known {
                lines.push("  ⚠ no se pudo comprobar la lista de idiomas de Tesseract".to_string());
            }
            for language in languages {
                if tools.languages_known && !tools.has_language(language) {
                    lines.push(format!(
                        "  ⚠ falta el idioma «{language}»: {}",
                        install_hint(ExternalTool::Tesseract)
                    ));
                }
            }
        }
        None => lines.push(format!(
            "Tesseract: no encontrado — {}",
            install_hint(ExternalTool::Tesseract)
        )),
    }

    match &tools.pdftoppm {
        Some(program) => lines.push(format!("Poppler (pdftoppm): {}", program.display())),
        None => lines.push(format!(
            "Poppler (pdftoppm): no encontrado — {}",
            install_hint(ExternalTool::Poppler)
        )),
    }

    match find_libreoffice() {
        Some(program) => lines.push(format!("LibreOffice: {}", program.display())),
        None => lines.push(format!(
            "LibreOffice: no encontrado — {}",
            install_hint(ExternalTool::LibreOffice)
        )),
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_informe_menciona_las_tres_herramientas() {
        let report = tools_report(&["spa".to_string()], 5);
        assert!(report.contains("Tesseract"), "{report}");
        assert!(report.contains("Poppler"), "{report}");
        assert!(report.contains("LibreOffice"), "{report}");
    }
}
