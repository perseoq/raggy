//! Construcción del prompt de sistema estricto (HU-14) con el contexto
//! recuperado y sus citas (HU-11), y la memoria de artefactos (HU-21).

/// Frase exacta que el modelo debe usar cuando el contexto no sirve.
pub const NOT_FOUND_MESSAGE: &str = "No encuentro esa información en los archivos indexados";

/// Instrucción de sistema que ancla al modelo al contexto recuperado.
pub const STRICT_SYSTEM_PROMPT: &str = "\
Eres un asistente técnico que responde con la documentación Markdown que el \
usuario ha indexado, entregada más abajo como «contexto recuperado».\n\
Reglas obligatorias:\n\
1. Responde únicamente con información del contexto. No inventes nombres de \
archivos, funciones, rutas, APIs ni comportamientos.\n\
2. Si el contexto responde a la pregunta, total o parcialmente, respóndela de \
forma directa y cita las fuentes con el formato [archivo:línea-línea].\n\
3. Si el contexto solo cubre una parte, responde esa parte y aclara con tus \
propias palabras qué queda fuera; no uses la frase de la regla 4.\n\
4. Solo si el contexto no tiene nada relacionado con la pregunta, responde \
únicamente: «No encuentro esa información en los archivos indexados».\n\
5. Nunca combines esa frase con una respuesta: o respondes con el contexto, o \
dices que no lo encuentras.\n\
6. Responde en el mismo idioma que la pregunta, de forma concisa y directa, sin \
repetir la pregunta ni copiar el contexto sin procesarlo.";

/// Longitud máxima del contenido de un artefacto incluido en el prompt.
const MAX_ARTIFACT_CHARS: usize = 4000;

/// Fragmento de contexto con su cita asociada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextBlock {
    /// Etiqueta de la cita, p. ej. `docs/guia.md:10-24`.
    pub citation: String,
    /// Texto del fragmento.
    pub text: String,
}

impl ContextBlock {
    /// Crea un bloque de contexto.
    pub fn new(citation: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            citation: citation.into(),
            text: text.into(),
        }
    }
}

/// Artefacto ya generado, para dar continuidad entre turnos (HU-21).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactContext {
    /// Nombre del archivo.
    pub name: String,
    /// Ruta relativa dentro del proyecto.
    pub relative_path: String,
    /// Lenguaje declarado, si se conoce.
    pub language: Option<String>,
    /// Contenido del archivo, incluido solo cuando es relevante.
    pub content: Option<String>,
}

impl ArtifactContext {
    /// Crea la referencia a un artefacto.
    pub fn new(
        name: impl Into<String>,
        relative_path: impl Into<String>,
        language: Option<String>,
        content: Option<String>,
    ) -> Self {
        Self {
            name: name.into(),
            relative_path: relative_path.into(),
            language,
            content,
        }
    }

    fn manifest_line(&self) -> String {
        match &self.language {
            Some(language) => format!("- {} ({language})", self.relative_path),
            None => format!("- {}", self.relative_path),
        }
    }
}

/// Compone el prompt de sistema con la instrucción estricta, el contexto y los
/// artefactos ya generados.
pub fn build_system_prompt(context: &[ContextBlock], artifacts: &[ArtifactContext]) -> String {
    let mut prompt = String::from(STRICT_SYSTEM_PROMPT);

    if context.is_empty() {
        prompt.push_str(
            "\n\nNo se recuperó ningún fragmento relevante del índice; si no puedes \
             responder con lo anterior, indica que no encuentras la información.",
        );
    } else {
        prompt.push_str("\n\nContexto recuperado:\n");
        for block in context {
            prompt.push_str(&format!("\n[Fuente: {}]\n{}\n", block.citation, block.text));
        }
    }

    if !artifacts.is_empty() {
        prompt.push_str(
            "\n\nArchivos que ya has generado en esta sesión (no los dupliques; \
             si el usuario pide modificar uno, parte de su contenido actual):\n",
        );
        for artifact in artifacts {
            prompt.push_str(&artifact.manifest_line());
            prompt.push('\n');
        }

        for artifact in artifacts
            .iter()
            .filter(|artifact| artifact.content.is_some())
        {
            let content = artifact.content.as_deref().unwrap_or_default();
            let truncated = truncate(content, MAX_ARTIFACT_CHARS);
            prompt.push_str(&format!(
                "\nContenido actual de {}:\n```{}\n{}\n```\n",
                artifact.relative_path,
                artifact.language.as_deref().unwrap_or(""),
                truncated
            ));
        }
    }

    prompt
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(limit).collect();
    truncated.push_str("\n… (recortado)");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_contexto_lo_advierte() {
        let prompt = build_system_prompt(&[], &[]);
        assert!(prompt.contains(STRICT_SYSTEM_PROMPT));
        assert!(prompt.contains("No se recuperó ningún fragmento"));
    }

    #[test]
    fn incluye_citas_y_texto() {
        let blocks = vec![ContextBlock::new("docs/guia.md:3-9", "contenido relevante")];
        let prompt = build_system_prompt(&blocks, &[]);
        assert!(prompt.contains("[Fuente: docs/guia.md:3-9]"));
        assert!(prompt.contains("contenido relevante"));
    }

    #[test]
    fn lista_artefactos_e_incluye_su_contenido() {
        let artifacts = vec![
            ArtifactContext::new("file_uno.py", "file_uno.py", Some("python".into()), None),
            ArtifactContext::new(
                "file_dos.py",
                "file_dos.py",
                Some("python".into()),
                Some("print('hola')".into()),
            ),
        ];
        let prompt = build_system_prompt(&[], &artifacts);
        assert!(prompt.contains("Archivos que ya has generado"));
        assert!(prompt.contains("- file_uno.py (python)"));
        assert!(prompt.contains("Contenido actual de file_dos.py"));
        assert!(prompt.contains("print('hola')"));
    }
}
