//! Utilidades para interpretar flujos *Server-Sent Events* (SSE).

/// Devuelve el contenido de los eventos `data:` de una línea SSE.
///
/// Se ignoran las líneas de comentario, los nombres de evento y las líneas
/// vacías; el tipo concreto del evento se deduce del JSON del campo `data`.
pub fn data_of(line: &str) -> Option<&str> {
    let line = line.trim_end_matches(['\r', '\n']);
    let data = line.strip_prefix("data:")?.trim_start();
    if data.is_empty() {
        None
    } else {
        Some(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrae_el_campo_data() {
        assert_eq!(data_of("data: {\"a\":1}"), Some("{\"a\":1}"));
        assert_eq!(data_of("data:[DONE]"), Some("[DONE]"));
        assert_eq!(data_of("event: message"), None);
        assert_eq!(data_of(": comentario"), None);
        assert_eq!(data_of("data:"), None);
    }
}
