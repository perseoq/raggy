//! Exportación de una sesión completa a Markdown (HU-31).

use std::path::{Path, PathBuf};

use raggy_storage::{Artifact, Message, Session};

use crate::error::{CoreError, Result};

/// Compone el Markdown de una sesión con sus metadatos, historial, citas y
/// artefactos generados.
pub fn session_markdown(
    session: &Session,
    folder_name: &str,
    messages: &[Message],
    artifacts: &[Artifact],
) -> String {
    let mut output = String::new();
    output.push_str(&format!("# {}\n\n", session.title));
    output.push_str(&format!("- Carpeta: {folder_name}\n"));
    output.push_str(&format!(
        "- Proveedor: {} · Modelo: {}\n",
        session.provider, session.model
    ));
    output.push_str(&format!("- Creada: {}\n", session.created_at));
    output.push_str(&format!("- Actualizada: {}\n\n", session.updated_at));

    output.push_str("## Conversación\n\n");
    for message in messages {
        let role = if message.role == "user" {
            "Tú"
        } else {
            "Asistente"
        };
        output.push_str(&format!("### {role}\n\n{}\n\n", message.content));

        if !message.citations.is_empty() {
            let labels: Vec<String> = message
                .citations
                .iter()
                .map(|citation| format!("`{}`", citation.label()))
                .collect();
            output.push_str(&format!("Fuentes: {}\n\n", labels.join(", ")));
        }
    }

    if !artifacts.is_empty() {
        output.push_str("## Artefactos generados\n\n");
        for artifact in artifacts {
            output.push_str(&format!(
                "### `{}`\n\n```{}\n{}\n```\n\n",
                artifact.relative_path,
                artifact.language.clone().unwrap_or_default(),
                artifact.content
            ));
        }
    }

    output
}

/// Escribe la exportación de una sesión en `directory` y devuelve la ruta.
pub fn write_session_export(
    directory: &Path,
    session: &Session,
    folder_name: &str,
    messages: &[Message],
    artifacts: &[Artifact],
) -> Result<PathBuf> {
    let markdown = session_markdown(session, folder_name, messages, artifacts);
    let path = directory.join(format!("{}.md", file_name(&session.title)));
    std::fs::write(&path, markdown).map_err(|source| CoreError::Io {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

/// Convierte el título de una sesión en un nombre de archivo seguro.
fn file_name(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "sesion".to_string()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("raggy-export-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("crea directorio temporal");
        path
    }

    #[test]
    fn incluye_metadatos_historial_citas_y_artefactos() {
        let session = Session::new("folder", "deepseek", "deepseek-chat");
        let mut messages = vec![Message::new(
            &session.id,
            "user",
            "¿Cómo compilo?",
            true,
            vec![],
        )];
        messages.push(Message::new(
            &session.id,
            "assistant",
            "Ejecuta cargo build.",
            true,
            vec![raggy_rag::Citation {
                source_file: "guia.md".to_string(),
                source_path: "docs/guia.md".to_string(),
                heading: None,
                start_line: 3,
                end_line: 9,
                score: 0.8,
            }],
        ));

        let artifacts = vec![Artifact::new(
            &session.id,
            "saludo.py",
            "saludo.py",
            "/tmp/saludo.py",
            Some("python".to_string()),
            "print('hola')",
            Some("deepseek-chat".to_string()),
        )];

        let markdown = session_markdown(&session, "docs", &messages, &artifacts);
        assert!(markdown.contains("- Carpeta: docs"));
        assert!(markdown.contains("Modelo: deepseek-chat"));
        assert!(markdown.contains("### Tú"));
        assert!(markdown.contains("### Asistente"));
        assert!(markdown.contains("`docs/guia.md:3-9`"));
        assert!(markdown.contains("## Artefactos generados"));
        assert!(markdown.contains("```python"));
        assert!(markdown.contains("print('hola')"));
    }

    #[test]
    fn escribe_el_archivo_con_nombre_seguro() {
        let dir = temp_dir("escritura");
        let session = Session::new("folder", "openai", "gpt-4o-mini");
        let path = write_session_export(&dir, &session, "docs", &[], &[]).expect("exporta");
        assert!(path.is_file());
        assert!(path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".md")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sanea_titulos_con_caracteres_especiales() {
        assert_eq!(file_name("2026-10-14 02:15"), "2026-10-14-02-15");
        assert_eq!(file_name("///"), "sesion");
    }
}
