//! OCR con Tesseract.
//!
//! Detecta el binario `tesseract` y sus idiomas, y rasteriza páginas de PDF con
//! `pdftoppm` (Poppler) para reconocerlas. Si falta alguna herramienta, se
//! informa de ello con la pista de instalación adecuada al sistema.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::tools::{install_hint, ExternalTool, Tools};
use crate::{ExtractError, Result};

/// Contexto de OCR: idiomas pedidos y herramientas detectadas.
#[derive(Debug, Clone)]
pub struct Ocr {
    languages: Vec<String>,
    tools: Tools,
    timeout_secs: u64,
}

impl Ocr {
    /// Detecta las herramientas de OCR disponibles en el sistema.
    pub fn detect(languages: &[String], timeout_secs: u64) -> Self {
        Self {
            languages: languages.to_vec(),
            tools: Tools::detect_with_timeout(timeout_secs),
            timeout_secs,
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
                if !self.tools.languages_known {
                    reasons.push(format!(
                        "no se pudo comprobar la lista de idiomas de Tesseract ({})",
                        install_hint(ExternalTool::Tesseract)
                    ));
                } else {
                    for language in &self.languages {
                        if !self.tools.has_language(language) {
                            reasons.push(format!(
                                "Tesseract no tiene el idioma «{language}» ({})",
                                install_hint(ExternalTool::Tesseract)
                            ));
                        }
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
    pub fn recognize_pdf_page(&self, pdf: &Path, page: u32, max_pixels: u64) -> Result<String> {
        let pdftoppm = self
            .tools
            .pdftoppm
            .as_ref()
            .ok_or_else(|| ExtractError::Parse("pdftoppm no está disponible".to_string()))?;

        let directory =
            crate::temp_directory("ocr").map_err(|error| ExtractError::Parse(error.to_string()))?;
        let prefix = directory.path().join("page");
        let page_number = page.to_string();
        let max_dimension = (max_pixels as f64).sqrt().floor().clamp(
            crate::limits::MIN_OCR_RENDER_DIMENSION as f64,
            crate::limits::MAX_OCR_RENDER_DIMENSION as f64,
        ) as u32;
        let max_dimension = max_dimension.to_string();

        let mut command = Command::new(pdftoppm);
        command
            .args([
                "-png",
                "-scale-to",
                &max_dimension,
                "-f",
                &page_number,
                "-l",
                &page_number,
            ])
            .arg(pdf)
            .arg(&prefix)
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let status = crate::run_with_timeout(&mut command, Duration::from_secs(self.timeout_secs));

        let result = match status {
            Ok(status) if status.success() => first_png(directory.path())
                .ok_or_else(|| ExtractError::Parse("pdftoppm no generó ninguna imagen".to_string()))
                .and_then(|image| self.recognize_image_in(&image, directory.path())),
            Ok(status) => Err(ExtractError::Parse(format!(
                "pdftoppm terminó con {status}"
            ))),
            Err(error) => Err(ExtractError::Parse(error.to_string())),
        };
        result
    }

    /// Aplica OCR a una imagen existente.
    pub fn recognize_image(&self, image: &Path) -> Result<String> {
        let directory = crate::temp_directory("tesseract")
            .map_err(|error| ExtractError::Parse(error.to_string()))?;
        self.recognize_image_in(image, directory.path())
    }

    /// Ejecuta Tesseract y guarda su salida solo dentro del directorio temporal
    /// privado de esta invocación.
    fn recognize_image_in(&self, image: &Path, directory: &Path) -> Result<String> {
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

        let output_path = directory.join("recognized.txt");
        let error_path = directory.join("tesseract.stderr");
        let stdout = std::fs::File::create(&output_path)
            .map_err(|error| ExtractError::Parse(error.to_string()))?;
        let stderr = std::fs::File::create(&error_path)
            .map_err(|error| ExtractError::Parse(error.to_string()))?;
        let mut command = Command::new(tesseract);
        command
            .arg(image)
            .arg("stdout")
            .arg("-l")
            .arg(&languages)
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        let status = crate::run_with_timeout(&mut command, Duration::from_secs(self.timeout_secs))
            .map_err(|error| ExtractError::Parse(error.to_string()))?;

        if !status.success() {
            let detail = std::fs::read_to_string(&error_path).unwrap_or_default();
            return Err(ExtractError::Parse(format!(
                "tesseract terminó con {status}: {}",
                detail.chars().take(400).collect::<String>()
            )));
        }

        std::fs::read_to_string(&output_path)
            .map_err(|error| ExtractError::Parse(error.to_string()))
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

    #[cfg(unix)]
    fn fake_program(directory: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = directory.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("crea herramienta falsa");
        let mut permissions = std::fs::metadata(&path)
            .expect("lee permisos")
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&path, permissions).expect("marca ejecutable");
        path
    }

    #[test]
    fn unavailability_es_coherente_con_is_ready() {
        let ocr = Ocr::detect(&["spa".to_string()], 5);
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
        let ocr = Ocr::detect(&["eng".to_string()], 5);
        let _ = ocr.recognize_image(Path::new("/tmp/raggy-no-existe.png"));
    }

    #[cfg(unix)]
    #[test]
    fn ejecuta_ocr_y_rasterizado_con_herramientas_falsas() {
        let directory = crate::temp_directory("ocr-falso").expect("crea entorno de prueba");
        let tesseract = fake_program(directory.path(), "tesseract", "printf 'texto simulado'");
        let pdftoppm = fake_program(
            directory.path(),
            "pdftoppm",
            "for prefix do :; done\nprintf 'png' > \"${prefix}-1.png\"",
        );
        let ocr = Ocr {
            languages: vec!["spa".to_string()],
            tools: Tools {
                tesseract: Some(tesseract),
                pdftoppm: Some(pdftoppm),
                languages: vec!["spa".to_string()],
                languages_known: true,
            },
            timeout_secs: 5,
        };
        let image = directory.path().join("pagina.png");
        std::fs::write(&image, b"imagen").expect("crea imagen sintética");

        assert_eq!(
            ocr.recognize_image(&image)
                .expect("ejecuta Tesseract falso"),
            "texto simulado"
        );
        assert_eq!(
            ocr.recognize_pdf_page(Path::new("documento.pdf"), 1, 1_000_000)
                .expect("ejecuta Poppler y Tesseract falsos"),
            "texto simulado"
        );
    }
}
