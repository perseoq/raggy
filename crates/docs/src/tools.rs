//! Detección de las herramientas externas que usa la extracción y pistas de
//! instalación por sistema operativo.
//!
//! El OCR se apoya en el binario `tesseract` (con su paquete de idioma, p. ej.
//! `spa`) y, para rasterizar páginas de PDF, en `pdftoppm` (Poppler). Los
//! formatos legacy `.doc`/`.ppt` se convierten con LibreOffice headless. Ninguna
//! se empaqueta con la aplicación: se detectan en el sistema y, si faltan, se
//! avisa con el comando de instalación adecuado.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Tiempo que se reutiliza el resultado de la detección. Evita lanzar
/// `tesseract --list-langs` una vez por carpeta o por PDF dentro de la misma
/// indexación, sin impedir que instalar una herramienta se detecte poco después.
const TOOLS_CACHE_TTL: Duration = Duration::from_secs(30);

fn cache() -> &'static Mutex<HashMap<u64, (Instant, Tools)>> {
    static CACHE: OnceLock<Mutex<HashMap<u64, (Instant, Tools)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Devuelve la detección memorizada si sigue vigente.
fn cached(timeout_secs: u64) -> Option<Tools> {
    let cache = cache().lock().ok()?;
    let (at, tools) = cache.get(&timeout_secs)?;
    (at.elapsed() < TOOLS_CACHE_TTL).then(|| tools.clone())
}

/// Descarta la detección memorizada para forzar una nueva (p. ej. tras instalar
/// una herramienta o al pulsar «Comprobar herramientas»).
pub fn invalidate_cache() {
    if let Ok(mut cache) = cache().lock() {
        cache.clear();
    }
}

/// Herramienta externa opcional.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalTool {
    /// Motor de OCR.
    Tesseract,
    /// Rasterizador de PDF (Poppler).
    Poppler,
    /// Suite ofimática para convertir formatos legacy.
    LibreOffice,
}

impl ExternalTool {
    /// Nombre del ejecutable buscado en el `PATH`.
    pub fn program(self) -> &'static str {
        match self {
            ExternalTool::Tesseract => "tesseract",
            ExternalTool::Poppler => "pdftoppm",
            ExternalTool::LibreOffice => "soffice",
        }
    }

    /// Variable de entorno que permite indicar una ruta explícita.
    pub fn env_var(self) -> &'static str {
        match self {
            ExternalTool::Tesseract => "RAGGY_TESSERACT",
            ExternalTool::Poppler => "RAGGY_PDFTOPPM",
            ExternalTool::LibreOffice => "RAGGY_SOFFICE",
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
    /// Si fue posible consultar la lista de idiomas de Tesseract.
    pub languages_known: bool,
}

impl Tools {
    /// Detecta las herramientas disponibles en el sistema.
    pub fn detect() -> Self {
        Self::detect_with_timeout(10)
    }

    /// Detecta herramientas; la consulta a `tesseract --list-langs` queda
    /// limitada para que el diagnóstico no pueda bloquear indefinidamente.
    ///
    /// El resultado se reutiliza durante [`TOOLS_CACHE_TTL`] para no repetir el
    /// subproceso en cada carpeta o documento.
    pub fn detect_with_timeout(timeout_secs: u64) -> Self {
        if let Some(tools) = cached(timeout_secs) {
            return tools;
        }
        let tools = Self::probe(timeout_secs);
        if let Ok(mut cache) = cache().lock() {
            cache.insert(timeout_secs, (Instant::now(), tools.clone()));
        }
        tools
    }

    /// Consulta real de las herramientas, sin caché.
    fn probe(timeout_secs: u64) -> Self {
        let probe_timeout = timeout_secs.clamp(1, crate::limits::MAX_TOOL_PROBE_TIMEOUT_SECS);
        let tesseract = find_program(ExternalTool::Tesseract);
        let language_result = tesseract
            .as_deref()
            .and_then(|program| tesseract_languages(program, probe_timeout));
        let languages_known = language_result.is_some();
        let languages = language_result.unwrap_or_default();
        Self {
            tesseract,
            pdftoppm: find_program(ExternalTool::Poppler),
            languages,
            languages_known,
        }
    }

    /// ¿Tesseract reconoce el idioma indicado?
    pub fn has_language(&self, language: &str) -> bool {
        self.languages.iter().any(|candidate| candidate == language)
    }

    /// ¿Está todo listo para OCR con los idiomas pedidos?
    pub fn ready_for(&self, languages: &[String]) -> bool {
        self.tesseract.is_some()
            && self.pdftoppm.is_some()
            && self.languages_known
            && languages.iter().all(|language| self.has_language(language))
    }
}

/// Busca un ejecutable en el `PATH` (o en su variable de entorno).
pub fn find_program(tool: ExternalTool) -> Option<PathBuf> {
    if let Some(explicit) = explicit_path(tool.env_var()) {
        return Some(explicit);
    }

    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        if let Some(found) = executable_in(&directory, tool.program()) {
            return Some(found);
        }
    }
    None
}

/// Localiza LibreOffice, que puede llamarse `soffice` o `libreoffice` y, en
/// macOS y Windows, vivir en rutas conocidas aunque no esté en el `PATH`.
pub fn find_libreoffice() -> Option<PathBuf> {
    if let Some(explicit) = explicit_path(ExternalTool::LibreOffice.env_var()) {
        return Some(explicit);
    }

    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            for name in ["soffice", "libreoffice"] {
                if let Some(found) = executable_in(&directory, name) {
                    return Some(found);
                }
            }
        }
    }

    known_libreoffice_paths()
        .into_iter()
        .find(|candidate| candidate.is_file())
}

/// Rutas típicas de LibreOffice fuera del `PATH`.
fn known_libreoffice_paths() -> Vec<PathBuf> {
    #[allow(unused_mut)]
    let mut paths = Vec::new();
    #[cfg(target_os = "macos")]
    paths.push(PathBuf::from(
        "/Applications/LibreOffice.app/Contents/MacOS/soffice",
    ));
    #[cfg(windows)]
    {
        for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Ok(base) = std::env::var(variable) {
                paths.push(Path::new(&base).join("LibreOffice\\program\\soffice.exe"));
            }
        }
    }
    paths
}

/// Devuelve la ruta de un ejecutable si existe (con `.exe` en Windows).
fn executable_in(directory: &Path, name: &str) -> Option<PathBuf> {
    let candidate = directory.join(name);
    if candidate.is_file() {
        return Some(candidate);
    }
    #[cfg(windows)]
    {
        let with_exe = directory.join(format!("{name}.exe"));
        if with_exe.is_file() {
            return Some(with_exe);
        }
    }
    None
}

/// Lee una ruta explícita de una variable de entorno, si es válida.
fn explicit_path(variable: &str) -> Option<PathBuf> {
    let value = std::env::var_os(variable)?;
    let path = PathBuf::from(value);
    if path.as_os_str().is_empty() {
        return None;
    }
    path.is_file().then_some(path)
}

/// Idiomas que reconoce `tesseract --list-langs`.
fn tesseract_languages(program: &Path, timeout_secs: u64) -> Option<Vec<String>> {
    let directory = crate::temp_directory("tesseract-probe").ok()?;
    let stdout_path = directory.path().join("stdout.txt");
    let stderr_path = directory.path().join("stderr.txt");
    let stdout = std::fs::File::create(&stdout_path).ok()?;
    let stderr = std::fs::File::create(&stderr_path).ok()?;
    let mut command = Command::new(program);
    command
        .arg("--list-langs")
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    let status = crate::run_with_timeout(&mut command, Duration::from_secs(timeout_secs)).ok()?;
    if !status.success() {
        return None;
    }

    let mut text = std::fs::read_to_string(stdout_path).unwrap_or_default();
    text.push('\n');
    text.push_str(&std::fs::read_to_string(stderr_path).unwrap_or_default());

    Some(
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .filter(|line| !line.to_ascii_lowercase().starts_with("list of"))
            .filter(|line| !line.contains(' ') && !line.contains('/') && !line.contains('\\'))
            .map(str::to_string)
            .collect(),
    )
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
            "instala Tesseract y los datos de idioma español (spa) desde los ports/packages"
                .to_string()
        }
        (ExternalTool::Tesseract, "macos") => "brew install tesseract tesseract-lang".to_string(),
        (ExternalTool::Tesseract, "windows") => {
            "instala Tesseract (compilación de UB Mannheim) y añade spa.traineddata a la carpeta \
             tessdata, o define TESSDATA_PREFIX"
                .to_string()
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

        (ExternalTool::LibreOffice, "debian") => "sudo apt install libreoffice".to_string(),
        (ExternalTool::LibreOffice, "fedora") => "sudo dnf install libreoffice".to_string(),
        (ExternalTool::LibreOffice, "arch") => "sudo pacman -S libreoffice-still".to_string(),
        (ExternalTool::LibreOffice, "suse") => "sudo zypper install libreoffice".to_string(),
        (ExternalTool::LibreOffice, "alpine") => "sudo apk add libreoffice".to_string(),
        (ExternalTool::LibreOffice, "bsd") => {
            "instala LibreOffice desde los ports/packages".to_string()
        }
        (ExternalTool::LibreOffice, "macos") => "brew install --cask libreoffice".to_string(),
        (ExternalTool::LibreOffice, "windows") => {
            "instala LibreOffice desde https://es.libreoffice.org/descarga/libreoffice/".to_string()
        }
        (ExternalTool::LibreOffice, _) => "instala LibreOffice".to_string(),
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
                "fedora", "rhel", "centos", "rocky", "alma", "amzn", "oracle",
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
        for tool in [
            ExternalTool::Tesseract,
            ExternalTool::Poppler,
            ExternalTool::LibreOffice,
        ] {
            assert!(!install_hint(tool).is_empty());
        }
    }

    #[test]
    fn la_familia_de_plataforma_es_conocida() {
        assert!(!platform_family().is_empty());
    }

    #[test]
    fn detectar_no_entra_en_panico() {
        let tools = Tools::detect();
        assert_eq!(
            tools.ready_for(&[]),
            tools.tesseract.is_some() && tools.pdftoppm.is_some() && tools.languages_known
        );
    }
}
