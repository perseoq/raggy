//! Guardado de artefactos en disco y memoria de proyecto (HU-17, HU-19, HU-21).
//!
//! Las rutas las declara el modelo, así que se sanean antes de escribir: se
//! descartan rutas absolutas, unidades y componentes `..` para que un artefacto
//! nunca se guarde fuera de la carpeta elegida.

use std::path::{Path, PathBuf};

use raggy_llm::ArtifactContext;
use raggy_storage::Artifact;

use crate::error::{CoreError, Result};

/// Política ante un archivo que ya existe (HU-17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    /// No sobrescribe: informa del conflicto.
    Fail,
    /// Sobrescribe el archivo existente.
    Overwrite,
    /// Guarda con un nombre alternativo.
    Rename,
}

/// Resultado de guardar un artefacto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    /// Se escribió en la ruta indicada.
    Written(PathBuf),
    /// Se escribió con un nombre alternativo por conflicto.
    Renamed(PathBuf),
    /// El archivo ya existía y la política era no sobrescribir.
    Exists(PathBuf),
}

impl SaveOutcome {
    /// Ruta final del archivo, si se escribió.
    pub fn path(&self) -> &Path {
        match self {
            SaveOutcome::Written(path) | SaveOutcome::Renamed(path) | SaveOutcome::Exists(path) => {
                path
            }
        }
    }
}

/// Sanea la ruta relativa declarada por el modelo.
///
/// Devuelve `None` si la ruta es absoluta, contiene `..`, unidad o está vacía.
pub fn sanitize_relative(relative: &str) -> Option<String> {
    let normalized = relative.trim().replace('\\', "/");
    let normalized = normalized
        .strip_prefix("./")
        .unwrap_or(&normalized)
        .to_string();

    if normalized.is_empty() || normalized.starts_with('/') || normalized.contains(':') {
        return None;
    }

    let mut parts = Vec::new();
    for part in normalized.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            other => parts.push(other),
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

/// Escribe un artefacto dentro de `root`, creando los subdirectorios necesarios.
pub fn save_artifact(
    root: &Path,
    relative: &str,
    content: &str,
    policy: ConflictPolicy,
) -> Result<SaveOutcome> {
    let relative = sanitize_relative(relative)
        .ok_or_else(|| CoreError::Artifact(format!("ruta no válida: {relative}")))?;
    let target = root.join(Path::new(&relative));

    if target.exists() {
        match policy {
            ConflictPolicy::Fail => return Ok(SaveOutcome::Exists(target)),
            ConflictPolicy::Overwrite => {
                write_file(&target, content)?;
                return Ok(SaveOutcome::Written(target));
            }
            ConflictPolicy::Rename => {
                let renamed = free_path(&target);
                write_file(&renamed, content)?;
                return Ok(SaveOutcome::Renamed(renamed));
            }
        }
    }

    write_file(&target, content)?;
    Ok(SaveOutcome::Written(target))
}

fn write_file(target: &Path, content: &str) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|source| CoreError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(target, content).map_err(|source| CoreError::Io {
        path: target.to_path_buf(),
        source,
    })
}

/// Devuelve un nombre de archivo libre añadiendo un sufijo numérico.
pub fn free_path(target: &Path) -> PathBuf {
    let parent = target.parent().unwrap_or_else(|| Path::new(""));
    let stem = target
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("archivo");
    let extension = target.extension().and_then(|extension| extension.to_str());

    for index in 1..10_000 {
        let name = match extension {
            Some(extension) => format!("{stem}_{index}.{extension}"),
            None => format!("{stem}_{index}"),
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }

    target.to_path_buf()
}

/// Ruta relativa de un archivo respecto de su raíz, normalizada con `/`.
pub fn relative_to(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Selecciona los artefactos que se ofrecerán al modelo (HU-21).
///
/// Siempre se incluye el manifiesto de los artefactos activos; el contenido solo
/// se añade cuando el usuario los menciona, para no inflar el contexto.
pub fn artifacts_for_prompt(artifacts: &[Artifact], user_message: &str) -> Vec<ArtifactContext> {
    let message = user_message.to_lowercase();

    artifacts
        .iter()
        .filter(|artifact| !artifact.archived)
        .map(|artifact| {
            let mentioned = message.contains(&artifact.name.to_lowercase())
                || message.contains(&artifact.relative_path.to_lowercase());
            let content = if mentioned {
                Some(artifact.content.clone())
            } else {
                None
            };
            ArtifactContext::new(
                artifact.name.clone(),
                artifact.relative_path.clone(),
                artifact.language.clone(),
                content,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("raggy-artifacts-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("crea directorio temporal");
        path
    }

    #[test]
    fn sanea_rutas_peligrosas() {
        assert_eq!(
            sanitize_relative("src/main.rs").as_deref(),
            Some("src/main.rs")
        );
        assert_eq!(sanitize_relative("./a/b.rs").as_deref(), Some("a/b.rs"));
        assert_eq!(sanitize_relative("a//b.rs").as_deref(), Some("a/b.rs"));

        assert_eq!(sanitize_relative("/etc/passwd"), None);
        assert_eq!(sanitize_relative("../../secreto"), None);
        assert_eq!(sanitize_relative("C:\\Windows\\x.dll"), None);
        assert_eq!(sanitize_relative("   "), None);
    }

    #[test]
    fn guarda_creando_subdirectorios() {
        let dir = temp_dir("save");
        let outcome =
            save_artifact(&dir, "src/lib.rs", "fn x() {}", ConflictPolicy::Fail).expect("guarda");
        assert_eq!(outcome, SaveOutcome::Written(dir.join("src/lib.rs")));
        assert_eq!(
            std::fs::read_to_string(dir.join("src/lib.rs")).expect("lee"),
            "fn x() {}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn respeta_la_politica_de_conflicto() {
        let dir = temp_dir("conflict");
        save_artifact(&dir, "uno.py", "v1", ConflictPolicy::Fail).expect("guarda");

        let exists = save_artifact(&dir, "uno.py", "v2", ConflictPolicy::Fail).expect("detecta");
        assert!(matches!(exists, SaveOutcome::Exists(_)));
        assert_eq!(
            std::fs::read_to_string(dir.join("uno.py")).expect("lee"),
            "v1"
        );

        save_artifact(&dir, "uno.py", "v3", ConflictPolicy::Overwrite).expect("sobrescribe");
        assert_eq!(
            std::fs::read_to_string(dir.join("uno.py")).expect("lee"),
            "v3"
        );

        let renamed =
            save_artifact(&dir, "uno.py", "v4", ConflictPolicy::Rename).expect("renombra");
        match renamed {
            SaveOutcome::Renamed(path) => {
                assert!(path.to_string_lossy().contains("uno_1.py"));
                assert_eq!(std::fs::read_to_string(path).expect("lee"), "v4");
            }
            other => panic!("se esperaba un renombrado, fue {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rechaza_rutas_que_escapan_de_la_raiz() {
        let dir = temp_dir("escape");
        let result = save_artifact(&dir, "../fuera.py", "x", ConflictPolicy::Fail);
        assert!(matches!(result, Err(CoreError::Artifact(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn incluye_contenido_solo_si_se_menciona_el_artefacto() {
        let artifacts = vec![
            Artifact::new(
                "s",
                "file_uno.py",
                "file_uno.py",
                "/tmp/file_uno.py",
                Some("python".into()),
                "print(1)",
                None,
            ),
            Artifact::new(
                "s",
                "file_dos.py",
                "file_dos.py",
                "/tmp/file_dos.py",
                Some("python".into()),
                "print(2)",
                None,
            ),
        ];

        let contexts = artifacts_for_prompt(&artifacts, "modifica file_uno.py");
        assert_eq!(contexts.len(), 2);
        assert!(contexts[0].content.is_some());
        assert!(contexts[1].content.is_none());

        let contexts = artifacts_for_prompt(&artifacts, "otra cosa");
        assert!(contexts.iter().all(|context| context.content.is_none()));
    }
}
