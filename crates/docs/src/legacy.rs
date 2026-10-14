//! Extracción de formatos legacy de Office (`.doc`, `.ppt`).
//!
//! Estos formatos binarios no tienen un lector fiable en Rust, así que se
//! convierten a su equivalente moderno (`.docx`, `.pptx`) con **LibreOffice
//! headless** si está instalado, y se reutilizan los extractores de Word y
//! PowerPoint. Si LibreOffice no está disponible, se devuelve un documento sin
//! secciones y con un aviso que incluye cómo instalarlo.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::tools::{find_libreoffice, install_hint, ExternalTool};
use crate::{slides, temp_directory, word, DocKind, ExtractError, ExtractedDocument, Result};

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
pub fn extract(path: &Path, target: LegacyTarget) -> Result<ExtractedDocument> {
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
    let converted = convert(&soffice, path, target, directory.path())?;
    dispatch(target, &converted)
}

/// Convierte el archivo con LibreOffice y devuelve la ruta del resultado.
fn convert(soffice: &Path, path: &Path, target: LegacyTarget, directory: &Path) -> Result<PathBuf> {
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

    let status = command
        .status()
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
fn dispatch(target: LegacyTarget, converted: &Path) -> Result<ExtractedDocument> {
    let bytes = crate::read_bytes(converted)?;
    match target {
        LegacyTarget::Word => word::extract(&bytes),
        LegacyTarget::Presentation => slides::extract(&bytes),
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
            LegacyTarget::Word,
        );
    }
}
