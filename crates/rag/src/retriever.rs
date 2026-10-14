//! Contrato de recuperación de contexto (HU-10).

use crate::chunk::Chunk;

/// Fragmento recuperado junto con su puntuación de similitud.
#[derive(Debug, Clone, PartialEq)]
pub struct RetrievedChunk {
    /// Fragmento original.
    pub chunk: Chunk,
    /// Puntuación de similitud en el rango `0.0..=1.0`.
    pub score: f32,
}

/// Parámetros de recuperación.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetrieverOptions {
    /// Número máximo de fragmentos a devolver.
    pub top_k: usize,
    /// Umbral mínimo de similitud para descartar ruido.
    pub min_score: f32,
}

impl Default for RetrieverOptions {
    fn default() -> Self {
        Self {
            top_k: 5,
            min_score: 0.3,
        }
    }
}

/// Contrato del recuperador de contexto.
pub trait Retriever: Send + Sync {
    /// Recupera los fragmentos más relevantes para `query`.
    ///
    /// Debe respetar `options.top_k` y descartar todo aquello por debajo de
    /// `options.min_score`.
    fn retrieve(&self, query: &str, options: &RetrieverOptions) -> Vec<RetrievedChunk>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_k_por_defecto_es_cinco() {
        assert_eq!(RetrieverOptions::default().top_k, 5);
    }
}
