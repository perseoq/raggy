//! Extracción de formatos legacy de Office (`.doc`, `.ppt`).
//!
//! Estos formatos binarios no tienen un lector fiable en Rust, así que se
//! convierten a su equivalente moderno (`.docx`, `.pptx`) con **LibreOffice
//! headless** si está instalado, y se reutilizan los extractores de Word y
//! PowerPoint. Si LibreOffice no está disponible, se devuelve un documento sin
//! secciones y con un aviso que incluye cómo instalarlo.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::tools::{find_libreoffice, install_hint, ExternalTool};
use crate::{
    read_bytes_limited, run_with_timeout, slides, temp_directory, word, DocKind, ExtractError,
    ExtractOptions, ExtractedDocument, Result,
};

/// Formato legacy que se quiere convertir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyTarget {
    /// Documento de Word (`.doc`).
    Word,
    /// Presentación (`.ppt`).
    Presentation,
}

impl LegacyTarget {
    /// Filtro de conversión de LibreOffice (= extensión de destino).
    fn filter(self) -> &'static str {
        match self {
            LegacyTarget::Word => "docx",
            LegacyTarget::Presentation => "pptx",
        }
    }

    /// Tipo de documento del resultado.
    fn kind(self) -> DocKind {
        match self {
            LegacyTarget::Word => DocKind::Word,
            LegacyTarget::Presentation => DocKind::PowerPoint,
        }
    }
}

/// Convierte y extrae un documento legacy.
pub fn extract(
    path: &Path,
    input_bytes: &[u8],
    target: LegacyTarget,
    options: &ExtractOptions,
) -> Result<ExtractedDocument> {
    let Some(soffice) = find_libreoffice() else {
        let mut document = ExtractedDocument::new(target.kind(), Vec::new());
        document.warnings.push(format!(
            "`{}` requiere LibreOffice para convertirse ({}).",
            file_name(path),
            install_hint(ExternalTool::LibreOffice)
        ));
        return Ok(document);
    };

    let directory =
        temp_directory("soffice").map_err(|error| ExtractError::Parse(error.to_string()))?;
    let input_name = path.file_name().unwrap_or_default();
    let input = directory.path().join(input_name);
    std::fs::write(&input, input_bytes).map_err(|error| ExtractError::Parse(error.to_string()))?;
    let converted = convert(
        &soffice,
        &input,
        target,
        directory.path(),
        options.process_timeout_secs,
    )?;
    let converted_bytes = read_bytes_limited(&converted, options.max_file_bytes)?;
    dispatch(target, &converted_bytes, options.max_archive_bytes)
}

/// Convierte el archivo con LibreOffice y devuelve la ruta del resultado.
fn convert(
    soffice: &Path,
    path: &Path,
    target: LegacyTarget,
    directory: &Path,
    timeout_secs: u64,
) -> Result<PathBuf> {
    let mut command = Command::new(soffice);
    command.arg("--headless").arg("--norestore");
    // Perfil aislado para no chocar con una instancia de LibreOffice abierta.
    #[cfg(not(windows))]
    command.arg(format!(
        "-env:UserInstallation=file://{}",
        directory.join("profile").display()
    ));
    command
        .arg("--convert-to")
        .arg(target.filter())
        .arg("--outdir")
        .arg(directory)
        .arg(path);

    command.stdout(Stdio::null()).stderr(Stdio::null());
    let status = run_with_timeout(&mut command, Duration::from_secs(timeout_secs))
        .map_err(|error| ExtractError::Parse(error.to_string()))?;
    if !status.success() {
        return Err(ExtractError::Parse(format!(
            "LibreOffice terminó con {status}"
        )));
    }

    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("documento");
    let expected = directory.join(format!("{stem}.{}", target.filter()));
    find_with_extension(directory, target.filter())
        .or_else(|| expected.is_file().then_some(expected))
        .ok_or_else(|| {
            ExtractError::Parse("LibreOffice no generó el archivo convertido".to_string())
        })
}

/// Extrae el archivo ya convertido con el extractor correspondiente.
fn dispatch(
    target: LegacyTarget,
    bytes: &[u8],
    max_archive_bytes: u64,
) -> Result<ExtractedDocument> {
    match target {
        LegacyTarget::Word => word::extract_with_limits(bytes, max_archive_bytes),
        LegacyTarget::Presentation => slides::extract_with_limits(bytes, max_archive_bytes),
    }
}

/// Primer archivo de un directorio con la extensión indicada.
fn find_with_extension(directory: &Path, extension: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(directory).ok()?.flatten() {
        let path = entry.path();
        let matches = path
            .extension()
            .and_then(|candidate| candidate.to_str())
            .map(|candidate| candidate.eq_ignore_ascii_case(extension))
            .unwrap_or(false);
        if matches {
            return Some(path);
        }
    }
    None
}

/// Nombre de archivo a partir de una ruta.
fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("documento")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapea_destinos() {
        assert_eq!(LegacyTarget::Word.filter(), "docx");
        assert_eq!(LegacyTarget::Word.kind(), DocKind::Word);
        assert_eq!(LegacyTarget::Presentation.filter(), "pptx");
        assert_eq!(LegacyTarget::Presentation.kind(), DocKind::PowerPoint);
    }

    #[test]
    fn encuentra_por_extension() {
        let directory = crate::temp_directory("legacy-test").expect("crea temporal");
        std::fs::write(directory.path().join("resultado.docx"), b"x").expect("escribe");
        assert!(find_with_extension(directory.path(), "docx").is_some());
        assert!(find_with_extension(directory.path(), "pptx").is_none());
    }

    #[test]
    fn un_archivo_inexistente_no_entra_en_panico() {
        // Según haya LibreOffice o no, devuelve un aviso o un error, sin pánico.
        let _ = extract(
            Path::new("/tmp/raggy-legacy-inexistente.doc"),
            b"legacy",
            LegacyTarget::Word,
            &ExtractOptions::default(),
        );
    }
}
