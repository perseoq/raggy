//! Tokenización e *hashing* determinista, base de los embeddings locales.

/// Tokeniza un texto: separa por caracteres no alfanuméricos, pasa a minúsculas
/// y conserva acentos (relevantes en español).
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for character in text.chars() {
        if character.is_alphanumeric() || character == '_' {
            current.extend(character.to_lowercase());
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

/// Hash FNV-1a de 64 bits, estable entre ejecuciones y plataformas.
pub fn fnv1a(value: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokeniza_acentos_y_separadores() {
        assert_eq!(
            tokenize("La Gestión, ágil funciona."),
            vec!["la", "gestión", "ágil", "funciona"]
        );
    }

    #[test]
    fn el_hash_es_estable() {
        assert_eq!(fnv1a("raggy"), fnv1a("raggy"));
        assert_ne!(fnv1a("raggy"), fnv1a("raggx"));
    }
}
