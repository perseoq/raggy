//! Búsqueda semántica sobre el índice de una carpeta (HU-10).
//!
//! Construye un recuperador híbrido (embeddings locales + BM25) con los
//! fragmentos persistidos y devuelve los más relevantes por encima del umbral.

use raggy_rag::{HashingEmbedding, HybridRetriever, RetrievedChunk, Retriever, RetrieverOptions};
use raggy_storage::Storage;

use crate::error::Result;

/// Cobertura de embeddings de una carpeta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbeddingCoverage {
    /// Fragmentos totales.
    pub chunks: usize,
    /// Fragmentos con embedding válido para la dimensionalidad actual.
    pub embedded: usize,
}

impl EmbeddingCoverage {
    /// ¿Conviene reindexar para regenerar los embeddings?
    pub fn needs_reindex(&self) -> bool {
        self.chunks > 0 && self.embedded < self.chunks
    }
}

/// Recupera los fragmentos más relevantes de una carpeta para `query`.
pub fn search_folder(
    storage: &Storage,
    folder_id: &str,
    query: &str,
    options: &RetrieverOptions,
    embedding_dimensions: usize,
) -> Result<Vec<RetrievedChunk>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let retriever = build_retriever(storage, folder_id, embedding_dimensions)?;
    Ok(retriever.retrieve(query, options))
}

/// Construye el recuperador híbrido a partir de los fragmentos almacenados.
pub fn build_retriever(
    storage: &Storage,
    folder_id: &str,
    embedding_dimensions: usize,
) -> Result<HybridRetriever> {
    let mut retriever = HybridRetriever::new(Box::new(HashingEmbedding::new(embedding_dimensions)));
    for (chunk, embedding) in storage.list_embedded_chunks(folder_id)? {
        retriever.add(chunk, embedding);
    }
    Ok(retriever)
}

/// Calcula la cobertura de embeddings de una carpeta.
pub fn embedding_coverage(
    storage: &Storage,
    folder_id: &str,
    embedding_dimensions: usize,
) -> Result<EmbeddingCoverage> {
    let chunks = storage.count_chunks(folder_id)? as usize;
    let embedded =
        storage.count_chunks_with_embedding_dimensions(folder_id, embedding_dimensions)? as usize;
    Ok(EmbeddingCoverage { chunks, embedded })
}
