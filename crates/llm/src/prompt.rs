//! Construcción del prompt de sistema estricto (HU-14) con el contexto
//! recuperado y sus citas (HU-11).

/// Instrucción de sistema que ancla al modelo al contexto recuperado.
pub const STRICT_SYSTEM_PROMPT: &str = "\
Eres un asistente técnico que responde EXCLUSIVAMENTE con base en el contexto \
recuperado que se te entrega más abajo, extraído de la documentación Markdown \
indexada por el usuario.\n\
Reglas obligatorias:\n\
1. No inventes nombres de archivos, funciones, rutas, APIs ni comportamientos.\n\
2. Si la respuesta no está en el contexto, responde exactamente: \
«No encuentro esa información en los archivos indexados».\n\
3. Cita siempre la fuente de cada afirmación indicando el archivo y las líneas \
entre corchetes, con el formato [archivo:línea-línea].\n\
4. Sé conciso y preciso; no añadas conocimiento externo.";

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

/// Compone el prompt de sistema con la instrucción estricta y el contexto.
pub fn build_system_prompt(blocks: &[ContextBlock]) -> String {
    let mut prompt = String::from(STRICT_SYSTEM_PROMPT);

    if blocks.is_empty() {
        prompt.push_str(
            "\n\nNo se recuperó ningún fragmento relevante del índice; si no puedes \
             responder con lo anterior, indica que no encuentras la información.",
        );
        return prompt;
    }

    prompt.push_str("\n\nContexto recuperado:\n");
    for block in blocks {
        prompt.push_str(&format!("\n[Fuente: {}]\n{}\n", block.citation, block.text));
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_contexto_lo_advierte() {
        let prompt = build_system_prompt(&[]);
        assert!(prompt.contains(STRICT_SYSTEM_PROMPT));
        assert!(prompt.contains("No se recuperó ningún fragmento"));
    }

    #[test]
    fn incluye_citas_y_texto() {
        let blocks = vec![ContextBlock::new("docs/guia.md:3-9", "contenido relevante")];
        let prompt = build_system_prompt(&blocks);
        assert!(prompt.contains("[Fuente: docs/guia.md:3-9]"));
        assert!(prompt.contains("contenido relevante"));
    }
}
