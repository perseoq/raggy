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

/// Extrae identificadores y referencias (códigos con letras y dígitos) de un
/// texto: p. ej. `V-2024-00953`, `C-2025-01819` o un RFC.
///
/// Se usan para buscar en la carpeta los registros que un archivo adjunto
/// referencia por su folio o clave (HU-34).
pub fn lookup_terms(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();

    for character in text.chars() {
        if character.is_alphanumeric() || matches!(character, '-' | '_' | '/' | '.') {
            current.push(character);
        } else {
            push_lookup(&mut terms, &mut current);
        }
    }
    push_lookup(&mut terms, &mut current);

    terms
}

/// Añade la candidata si parece una referencia (letras y varios dígitos).
fn push_lookup(terms: &mut Vec<String>, current: &mut String) {
    let candidate = std::mem::take(current);
    let candidate = candidate.trim_matches(|character| matches!(character, '-' | '_' | '/' | '.'));
    let digits = candidate.chars().filter(char::is_ascii_digit).count();
    let letters = candidate
        .chars()
        .filter(|character| character.is_alphabetic())
        .count();
    // Se exigen letras y al menos dos dígitos: descarta años, fechas e importes.
    if candidate.chars().count() >= 5 && digits >= 2 && letters >= 1 {
        let normalized = candidate.to_lowercase();
        if !terms.contains(&normalized) {
            terms.push(normalized);
        }
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

    #[test]
    fn extrae_referencias_y_descarta_fechas_e_importes() {
        let reference = "Referencia ventas.csv: V-2024-00953, RFC VUX170518WQG, \
                         folio FC-2025-003, fecha 2025-01-01, total 1160.00";
        assert_eq!(
            lookup_terms(reference),
            vec![
                "v-2024-00953".to_string(),
                "vux170518wqg".to_string(),
                "fc-2025-003".to_string()
            ]
        );
        assert!(lookup_terms("solo texto sin códigos").is_empty());
        assert!(lookup_terms("2025-01-01").is_empty());
    }
}
