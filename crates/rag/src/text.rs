//! Tokenización e *hashing* determinista, base de los embeddings locales.

use crate::stopwords::is_stopword;

/// Tokeniza un texto para indexar y buscar: separa por caracteres no
/// alfanuméricos, pasa a minúsculas, elimina acentos y descarta palabras vacías.
///
/// Normalizar los acentos mejora la coincidencia cuando el usuario escribe sin
/// tildes («gestion» debe encontrar «gestión»).
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for character in text.chars() {
        if character.is_alphanumeric() || character == '_' {
            for lower in character.to_lowercase() {
                current.push(fold(lower));
            }
        } else if !current.is_empty() {
            push_token(&mut tokens, &mut current);
        }
    }

    if !current.is_empty() {
        push_token(&mut tokens, &mut current);
    }

    tokens
}

/// Añade el token si no es una palabra vacía y limpia el acumulador.
fn push_token(tokens: &mut Vec<String>, current: &mut String) {
    if !is_stopword(current) {
        tokens.push(std::mem::take(current));
    } else {
        current.clear();
    }
}

/// Reduce las vocales acentuadas y algunas consonantes a su forma básica.
fn fold(character: char) -> char {
    match character {
        'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => 'a',
        'é' | 'è' | 'ë' | 'ê' => 'e',
        'í' | 'ì' | 'ï' | 'î' => 'i',
        'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
        'ú' | 'ù' | 'ü' | 'û' => 'u',
        'ñ' => 'n',
        'ç' => 'c',
        other => other,
    }
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
    fn normaliza_acentos_y_quita_palabras_vacias() {
        assert_eq!(
            tokenize("La Gestión, ágil funciona."),
            vec!["gestion", "agil", "funciona"]
        );
    }

    #[test]
    fn el_acento_no_impide_la_coincidencia() {
        assert_eq!(tokenize("gestión"), tokenize("gestion"));
        assert_eq!(tokenize("AÑO"), tokenize("ano"));
    }

    #[test]
    fn el_hash_es_estable() {
        assert_eq!(fnv1a("raggy"), fnv1a("raggy"));
        assert_ne!(fnv1a("raggy"), fnv1a("raggx"));
    }
}
