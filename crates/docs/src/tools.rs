//! Detección de las herramientas externas que usa el OCR y pistas de
//! instalación por sistema operativo.
//!
//! El OCR se apoya en el binario `tesseract` (con su paquete de idioma, p. ej.
//! `spa`) y, para rasterizar páginas de PDF, en `pdftoppm` (Poppler). Ninguno se
//! empaqueta con la aplicación: se detectan en el sistema y, si faltan, se avisa
//! con el comando de instalación adecuado.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Herramienta externa opcional.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalTool {
    /// Motor de OCR.
    Tesseract,
    /// Rasterizador de PDF (Poppler).
    Poppler,
}

impl ExternalTool {
    /// Nombre del ejecutable buscado en el `PATH`.
    pub fn program(self) -> &'static str {
        match self {
            ExternalTool::Tesseract => "tesseract",
            ExternalTool::Poppler => "pdftoppm",
        }
    }

    /// Variable de entorno que permite indicar una ruta explícita.
    pub fn env_var(self) -> &'static str {
        match self {
            ExternalTool::Tesseract => "RAGGY_TESSERACT",
            ExternalTool::Poppler => "RAGGY_PDFTOPPM",
        }
    }
}

/// Estado de detección de las herramientas externas.
#[derive(Debug, Clone, Default)]
pub struct Tools {
    /// Ruta a `tesseract`, si está disponible.
    pub tesseract: Option<PathBuf>,
    /// Ruta a `pdftoppm`, si está disponible.
    pub pdftoppm: Option<PathBuf>,
    /// Idiomas que reconoce Tesseract (vacío si no se pudo consultar).
    pub languages: Vec<String>,
}

impl Tools {
    /// Detecta las herramientas disponibles en el sistema.
    pub fn detect() -> Self {
        let tesseract = find_program(ExternalTool::Tesseract);
        let languages = tesseract
            .as_deref()
            .map(tesseract_languages)
            .unwrap_or_default();
        Self {
            tesseract,
            pdftoppm: find_program(ExternalTool::Poppler),
            languages,
        }
    }

    /// ¿Tesseract reconoce el idioma indicado?
    pub fn has_language(&self, language: &str) -> bool {
        self.languages.iter().any(|candidate| candidate == language)
    }

    /// ¿Está todo listo para OCR con los idiomas pedidos?
    pub fn ready_for(&self, languages: &[String]) -> bool {
        if self.tesseract.is_none() {
            return false;
        }
        // Si no se pudo listar idiomas, se intenta igualmente.
        self.languages.is_empty() || languages.iter().all(|l| self.has_language(l))
    }
}

/// Busca un ejecutable en el `PATH` (o en su variable de entorno).
pub fn find_program(tool: ExternalTool) -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var(tool.env_var()) {
        let explicit = explicit.trim();
        if !explicit.is_empty() {
            let path = PathBuf::from(explicit);
            if path.is_file() {
                return Some(path);
            }
        }
    }

    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(tool.program());
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let with_exe = directory.join(format!("{}.exe", tool.program()));
            if with_exe.is_file() {
                return Some(with_exe);
            }
        }
    }
    None
}

/// Idiomas que reconoce `tesseract --list-langs`.
fn tesseract_languages(program: &Path) -> Vec<String> {
    let Ok(output) = Command::new(program).arg("--list-langs").output() else {
        return Vec::new();
    };

    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&output.stderr));

    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.to_ascii_lowercase().starts_with("list of"))
        .filter(|line| !line.contains(' ') && !line.contains('/') && !line.contains('\\'))
        .map(str::to_string)
        .collect()
}

/// Comando de instalación recomendado para una herramienta (con el paquete de
/// idioma español en el caso de Tesseract).
pub fn install_hint(tool: ExternalTool) -> String {
    match (tool, platform_family()) {
        (ExternalTool::Tesseract, "debian") => {
            "sudo apt install tesseract-ocr tesseract-ocr-spa".to_string()
        }
        (ExternalTool::Tesseract, "fedora") => {
            "sudo dnf install tesseract tesseract-langpack-spa".to_string()
        }
        (ExternalTool::Tesseract, "arch") => {
            "sudo pacman -S tesseract tesseract-data-spa".to_string()
        }
        (ExternalTool::Tesseract, "suse") => {
            "sudo zypper install tesseract-ocr tesseract-ocr-traineddata-spanish".to_string()
        }
        (ExternalTool::Tesseract, "alpine") => {
            "sudo apk add tesseract-ocr tesseract-ocr-data-spa".to_string()
        }
        (ExternalTool::Tesseract, "bsd") => {
            "instala Tesseract y los datos de idioma español (spa) desde los ports/packages".to_string()
        }
        (ExternalTool::Tesseract, "macos") => "brew install tesseract tesseract-lang".to_string(),
        (ExternalTool::Tesseract, "windows") => {
            "instala Tesseract (compilación de UB Mannheim) y añade spa.traineddata a la carpeta tessdata, o define TESSDATA_PREFIX".to_string()
        }
        (ExternalTool::Tesseract, _) => {
            "instala Tesseract y el paquete de idioma español (spa)".to_string()
        }

        (ExternalTool::Poppler, "debian") => "sudo apt install poppler-utils".to_string(),
        (ExternalTool::Poppler, "fedora") => "sudo dnf install poppler-utils".to_string(),
        (ExternalTool::Poppler, "arch") => "sudo pacman -S poppler".to_string(),
        (ExternalTool::Poppler, "suse") => "sudo zypper install poppler-tools".to_string(),
        (ExternalTool::Poppler, "alpine") => "sudo apk add poppler-utils".to_string(),
        (ExternalTool::Poppler, "bsd") => {
            "instala las herramientas de Poppler (pdftoppm) desde los ports/packages".to_string()
        }
        (ExternalTool::Poppler, "macos") => "brew install poppler".to_string(),
        (ExternalTool::Poppler, "windows") => {
            "descarga los binarios de Poppler para Windows y añádelos al PATH".to_string()
        }
        (ExternalTool::Poppler, _) => "instala las herramientas de Poppler (pdftoppm)".to_string(),
    }
}

/// Familia de plataforma para elegir las instrucciones de instalación.
fn platform_family() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        "freebsd" | "openbsd" | "netbsd" | "dragonfly" => "bsd",
        "linux" => linux_family(),
        _ => "linux",
    }
}

/// Familia de la distribución de Linux, leída de `/etc/os-release`.
fn linux_family() -> &'static str {
    let Ok(contents) = std::fs::read_to_string("/etc/os-release") else {
        return "linux";
    };
    let mut id = String::new();
    let mut like = String::new();
    for line in contents.lines() {
        if let Some(value) = line.strip_prefix("ID=") {
            id = value.trim_matches('"').to_ascii_lowercase();
        } else if let Some(value) = line.strip_prefix("ID_LIKE=") {
            like = value.trim_matches('"').to_ascii_lowercase();
        }
    }
    let combined = format!("{id} {like}");
    for (family, needles) in [
        ("debian", ["debian", "ubuntu", "mint", "pop"].as_slice()),
        (
            "fedora",
            [
                "fedora", "rhel", "centos", "rocky", "alma", "amzn", "ol", "oracle",
            ]
            .as_slice(),
        ),
        ("arch", ["arch", "manjaro", "endeavouros"].as_slice()),
        ("suse", ["opensuse", "suse", "sles"].as_slice()),
        ("alpine", ["alpine"].as_slice()),
    ] {
        if needles.iter().any(|needle| combined.contains(needle)) {
            return family;
        }
    }
    "linux"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn las_pistas_de_instalacion_no_estan_vacias() {
        assert!(!install_hint(ExternalTool::Tesseract).is_empty());
        assert!(!install_hint(ExternalTool::Poppler).is_empty());
    }

    #[test]
    fn la_familia_de_plataforma_es_conocida() {
        assert!(!platform_family().is_empty());
    }

    #[test]
    fn detectar_no_entra_en_panico() {
        let tools = Tools::detect();
        assert_eq!(tools.ready_for(&[]), tools.tesseract.is_some());
    }
}
