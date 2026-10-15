//! Búsqueda semántica sobre el índice de una carpeta (HU-10).
//!
//! Construye un recuperador híbrido (embeddings locales + BM25) con los
//! fragmentos persistidos y devuelve los más relevantes por encima del umbral.
//! La interfaz reutiliza `SearchCache` entre consultas y lo renueva cuando cambia
//! la revisión de la carpeta.

use std::collections::HashMap;

use raggy_rag::{HashingEmbedding, HybridRetriever, RetrievedChunk, Retriever, RetrieverOptions};
use raggy_storage::Storage;

use crate::error::Result;

struct CachedRetriever {
    updated_at: String,
    retriever: HybridRetriever,
}

/// Recuperadores reutilizables por carpeta y dimensionalidad (HU-10).
#[derive(Default)]
pub struct SearchCache {
    entries: HashMap<(String, usize), CachedRetriever>,
}

impl SearchCache {
    /// Busca con un índice ya construido o lo crea si cambió la carpeta.
    pub fn search_folder(
        &mut self,
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

        let key = (folder_id.to_string(), embedding_dimensions);
        let Some(folder) = storage.get_folder(folder_id)? else {
            self.invalidate_folder(folder_id);
            return Ok(Vec::new());
        };
        let needs_rebuild = self
            .entries
            .get(&key)
            .is_none_or(|cached| cached.updated_at != folder.updated_at);
        if needs_rebuild {
            let retriever = build_retriever(storage, folder_id, embedding_dimensions)?;
            self.entries.retain(|(id, dimensions), _| {
                id != folder_id || *dimensions == embedding_dimensions
            });
            self.entries.insert(
                key.clone(),
                CachedRetriever {
                    updated_at: folder.updated_at,
                    retriever,
                },
            );
        }

        Ok(self
            .entries
            .get(&key)
            .expect("se construyó el recuperador que faltaba")
            .retriever
            .retrieve(query, options))
    }

    /// Descarta todas las dimensionalidades cacheadas para una carpeta borrada.
    pub fn invalidate_folder(&mut self, folder_id: &str) {
        self.entries.retain(|(id, _), _| id != folder_id);
    }

    /// Descarta todos los recuperadores.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

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

/// Recupera los fragmentos más relevantes para una consulta aislada.
/// En flujos interactivos con consultas repetidas, conserva un [`SearchCache`]
/// para no reconstruir el recuperador desde SQLite en cada petición.
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

#[cfg(test)]
mod tests {
    use super::*;
    use raggy_rag::ChunkMetadata;
    use raggy_storage::{Folder, FolderStatus};

    fn chunk(id: &str, content: &str) -> raggy_rag::Chunk {
        raggy_rag::Chunk {
            id: id.to_string(),
            content_hash: id.to_string(),
            content: content.to_string(),
            metadata: ChunkMetadata {
                source_file: "guia.md".to_string(),
                source_path: "guia.md".to_string(),
                heading: None,
                location: None,
                start_line: 1,
                end_line: 1,
            },
        }
    }

    #[test]
    fn reutiliza_el_indice_hasta_que_cambia_la_revision_de_la_carpeta() {
        let mut storage = Storage::open_in_memory().expect("abre SQLite");
        let folder = Folder::new("/tmp/busqueda", "búsqueda");
        storage.upsert_folder(&folder).expect("guarda carpeta");
        storage
            .replace_chunks(
                &folder.id,
                "guia.md",
                &[chunk("primero", "alfa")],
                &[vec![0.0; 32]],
            )
            .expect("guarda chunk inicial");

        let mut cache = SearchCache::default();
        let options = RetrieverOptions {
            top_k: 5,
            min_score: 0.01,
        };
        let initial = cache
            .search_folder(&storage, &folder.id, "alfa", &options, 32)
            .expect("busca el índice inicial");
        assert_eq!(initial.len(), 1);

        storage
            .replace_chunks(
                &folder.id,
                "guia.md",
                &[chunk("segundo", "beta")],
                &[vec![0.0; 32]],
            )
            .expect("cambia chunks sin cerrar la indexación");
        let cached = cache
            .search_folder(&storage, &folder.id, "beta", &options, 32)
            .expect("busca el índice cacheado");
        assert!(cached.is_empty(), "no reconstruye en cada consulta");

        storage
            .set_folder_status(&folder.id, FolderStatus::Indexed, 1)
            .expect("publica una nueva revisión de índice");
        let refreshed = cache
            .search_folder(&storage, &folder.id, "beta", &options, 32)
            .expect("reconstruye tras la reindexación");
        assert_eq!(refreshed.len(), 1);
        assert_eq!(refreshed[0].chunk.id, "segundo");
    }
}
