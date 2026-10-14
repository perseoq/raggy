//! OCR con Tesseract.
//!
//! Detecta el binario `tesseract` y sus idiomas, y rasteriza páginas de PDF con
//! `pdftoppm` (Poppler) para reconocerlas. Si falta alguna herramienta, se
//! informa de ello con la pista de instalación adecuada al sistema.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::tools::{install_hint, ExternalTool, Tools};
use crate::{ExtractError, Result};

/// Resolución de rasterizado de una página de PDF, en puntos por pulgada.
const PDF_DPI: &str = "300";

/// Contexto de OCR: idiomas pedidos y herramientas detectadas.
#[derive(Debug, Clone)]
pub struct Ocr {
    languages: Vec<String>,
    tools: Tools,
}

impl Ocr {
    /// Detecta las herramientas de OCR disponibles en el sistema.
    pub fn detect(languages: &[String]) -> Self {
        Self {
            languages: languages.to_vec(),
            tools: Tools::detect(),
        }
    }

    /// ¿Está todo listo para OCR con los idiomas pedidos?
    pub fn is_ready(&self) -> bool {
        self.tools.ready_for(&self.languages)
    }

    /// Idiomas configurados.
    pub fn languages(&self) -> &[String] {
        &self.languages
    }

    /// Estado de las herramientas detectadas.
    pub fn tools(&self) -> &Tools {
        &self.tools
    }

    /// Motivo por el que el OCR no está disponible (con pistas de instalación),
    /// o `None` si sí lo está.
    pub fn unavailability(&self) -> Option<String> {
        if self.is_ready() {
            return None;
        }

        let mut reasons = Vec::new();
        match &self.tools.tesseract {
            None => reasons.push(format!(
                "falta Tesseract ({})",
                install_hint(ExternalTool::Tesseract)
            )),
            Some(_) => {
                for language in &self.languages {
                    if !self.tools.languages.is_empty() && !self.tools.has_language(language) {
                        reasons.push(format!(
                            "Tesseract no tiene el idioma «{language}» ({})",
                            install_hint(ExternalTool::Tesseract)
                        ));
                    }
                }
            }
        }

        if self.tools.pdftoppm.is_none() {
            reasons.push(format!(
                "falta Poppler/pdftoppm ({})",
                install_hint(ExternalTool::Poppler)
            ));
        }

        Some(reasons.join("; "))
    }

    /// Reconoce una página de PDF: la rasteriza con `pdftoppm` y aplica OCR.
    pub fn recognize_pdf_page(&self, pdf: &Path, page: u32) -> Result<String> {
        let pdftoppm = self
            .tools
            .pdftoppm
            .as_ref()
            .ok_or_else(|| ExtractError::Parse("pdftoppm no está disponible".to_string()))?;

        let directory =
            crate::temp_directory("ocr").map_err(|error| ExtractError::Parse(error.to_string()))?;
        let prefix = directory.path().join("page");
        let page_number = page.to_string();

        let status = Command::new(pdftoppm)
            .args([
                "-png",
                "-r",
                PDF_DPI,
                "-f",
                &page_number,
                "-l",
                &page_number,
            ])
            .arg(pdf)
            .arg(&prefix)
            .status();

        let result = match status {
            Ok(status) if status.success() => first_png(directory.path())
                .ok_or_else(|| ExtractError::Parse("pdftoppm no generó ninguna imagen".to_string()))
                .and_then(|image| self.recognize_image(&image)),
            Ok(status) => Err(ExtractError::Parse(format!(
                "pdftoppm terminó con {status}"
            ))),
            Err(error) => Err(ExtractError::Parse(error.to_string())),
        };
        result
    }

    /// Aplica OCR a una imagen existente.
    pub fn recognize_image(&self, image: &Path) -> Result<String> {
        let tesseract = self
            .tools
            .tesseract
            .as_ref()
            .ok_or_else(|| ExtractError::Parse("tesseract no está disponible".to_string()))?;

        let languages = if self.languages.is_empty() {
            "eng".to_string()
        } else {
            self.languages.join("+")
        };

        let output = Command::new(tesseract)
            .arg(image)
            .arg("stdout")
            .arg("-l")
            .arg(&languages)
            .output()
            .map_err(|error| ExtractError::Parse(error.to_string()))?;

        if !output.status.success() {
            return Err(ExtractError::Parse(format!(
                "tesseract terminó con {}",
                output.status
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

/// Primera imagen PNG de un directorio.
fn first_png(directory: &Path) -> Option<PathBuf> {
    for entry in std::fs::read_dir(directory).ok()?.flatten() {
        let path = entry.path();
        let is_png = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.eq_ignore_ascii_case("png"))
            .unwrap_or(false);
        if is_png {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailability_es_coherente_con_is_ready() {
        let ocr = Ocr::detect(&["spa".to_string()]);
        if ocr.is_ready() {
            assert!(ocr.unavailability().is_none());
        } else {
            assert!(ocr.unavailability().is_some());
        }
    }

    #[test]
    fn reconoce_una_imagen_inexistente_sin_panico() {
        // Si no hay Tesseract, devuelve error; si lo hay, también (la imagen no
        // existe). En ningún caso debe entrar en pánico.
        let ocr = Ocr::detect(&["eng".to_string()]);
        let _ = ocr.recognize_image(Path::new("/tmp/raggy-no-existe.png"));
    }
}
