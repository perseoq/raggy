//! Recuperación de contexto: fusión híbrida de similitud vectorial y BM25.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::bm25::Bm25Index;
use crate::chunk::Chunk;
use crate::embedding::EmbeddingProvider;
use crate::vector::VectorIndex;

/// Cita verificable de un fragmento (HU-11).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Citation {
    /// Nombre del archivo de origen.
    pub source_file: String,
    /// Ruta relativa a la carpeta indexada.
    pub source_path: String,
    /// Encabezado padre, si lo hay.
    pub heading: Option<String>,
    /// Ubicación legible en el documento original (p. ej. `pág. 3`), si aplica.
    #[serde(default)]
    pub location: Option<String>,
    /// Primera línea citada (1-based).
    pub start_line: usize,
    /// Última línea citada (1-based).
    pub end_line: usize,
    /// Puntuación de recuperación.
    pub score: f32,
}

impl Citation {
    /// Construye la cita de un fragmento.
    pub fn from_chunk(chunk: &Chunk, score: f32) -> Self {
        Self {
            source_file: chunk.metadata.source_file.clone(),
            source_path: chunk.metadata.source_path.clone(),
            heading: chunk.metadata.heading.clone(),
            location: chunk.metadata.location.clone(),
            start_line: chunk.metadata.start_line,
            end_line: chunk.metadata.end_line,
            score,
        }
    }

    /// Etiqueta legible, p. ej. `docs/guia.md:10-24` o `datos.csv (filas 1-100)`.
    pub fn label(&self) -> String {
        match &self.location {
            Some(location) => format!("{} ({location})", self.source_path),
            None => format!("{}:{}-{}", self.source_path, self.start_line, self.end_line),
        }
    }
}

/// Fragmento recuperado junto con sus puntuaciones.
#[derive(Debug, Clone, PartialEq)]
pub struct RetrievedChunk {
    /// Fragmento original.
    pub chunk: Chunk,
    /// Puntuación combinada usada para ordenar (en `0.0..=1.0`).
    pub score: f32,
    /// Similitud vectorial normalizada a `0.0..=1.0`.
    pub vector_score: f32,
    /// Puntuación léxica (BM25) normalizada a `0.0..=1.0`.
    pub lexical_score: f32,
}

impl RetrievedChunk {
    /// Cita verificable del fragmento (HU-11).
    pub fn citation(&self) -> Citation {
        Citation::from_chunk(&self.chunk, self.score)
    }
}

/// Parámetros de recuperación.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetrieverOptions {
    /// Número máximo de fragmentos a devolver.
    pub top_k: usize,
    /// Umbral mínimo de la puntuación combinada para descartar ruido.
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
    fn retrieve(&self, query: &str, options: &RetrieverOptions) -> Vec<RetrievedChunk>;
}

/// Peso de la componente vectorial en la fusión (el resto es BM25).
pub const DEFAULT_VECTOR_WEIGHT: f32 = 0.5;

/// Recuperador híbrido que combina embeddings locales y BM25.
pub struct HybridRetriever {
    embedder: Box<dyn EmbeddingProvider>,
    vector: VectorIndex,
    lexical: Bm25Index,
    chunks: HashMap<String, Chunk>,
    vector_weight: f32,
}

impl HybridRetriever {
    /// Crea el recuperador a partir de un proveedor de embeddings.
    pub fn new(embedder: Box<dyn EmbeddingProvider>) -> Self {
        let dimensions = embedder.dimensions();
        Self {
            embedder,
            vector: VectorIndex::new(dimensions),
            lexical: Bm25Index::new(),
            chunks: HashMap::new(),
            vector_weight: DEFAULT_VECTOR_WEIGHT,
        }
    }

    /// Añade un fragmento con su embedding (opcional).
    ///
    /// Si el embedding falta o su dimensionalidad no coincide con la del
    /// proveedor, el fragmento sigue siendo recuperable por BM25.
    pub fn add(&mut self, chunk: Chunk, embedding: Option<Vec<f32>>) {
        self.lexical.add(chunk.id.clone(), &chunk.content);
        if let Some(vector) = embedding {
            self.vector.add(chunk.id.clone(), vector);
        }
        self.chunks.insert(chunk.id.clone(), chunk);
    }

    /// Número de fragmentos indexados.
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// ¿Está vacío el recuperador?
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Número de fragmentos con embedding utilizable.
    pub fn embedded_len(&self) -> usize {
        self.vector.len()
    }
}

impl Retriever for HybridRetriever {
    fn retrieve(&self, query: &str, options: &RetrieverOptions) -> Vec<RetrievedChunk> {
        if self.chunks.is_empty() || query.trim().is_empty() {
            return Vec::new();
        }

        let pool = options.top_k.saturating_mul(4).max(16);

        let query_embedding = self.embedder.embed(query).vector;
        let vector_hits = self.vector.search(&query_embedding, pool, f32::MIN);
        let lexical_hits = self.lexical.search(query, pool);

        let lexical_max = lexical_hits
            .first()
            .map(|(_, score)| *score)
            .unwrap_or(0.0)
            .max(f32::EPSILON);

        let mut fused: HashMap<String, (f32, f32)> = HashMap::new();
        for (id, score) in vector_hits {
            fused.entry(id).or_insert((0.0, 0.0)).0 = score.clamp(0.0, 1.0);
        }
        for (id, score) in lexical_hits {
            fused.entry(id).or_insert((0.0, 0.0)).1 = (score / lexical_max).clamp(0.0, 1.0);
        }

        let mut results: Vec<RetrievedChunk> = fused
            .into_iter()
            .filter_map(|(id, (vector_score, lexical_score))| {
                let chunk = self.chunks.get(&id)?.clone();
                let score =
                    self.vector_weight * vector_score + (1.0 - self.vector_weight) * lexical_score;
                Some(RetrievedChunk {
                    chunk,
                    score,
                    vector_score,
                    lexical_score,
                })
            })
            .filter(|result| result.score >= options.min_score)
            .collect();

        results.sort_by(|a, b| b.score.total_cmp(&a.score));
        results.truncate(options.top_k);
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::ChunkMetadata;
    use crate::embedding::HashingEmbedding;

    fn chunk(id: &str, content: &str) -> Chunk {
        Chunk {
            id: id.to_string(),
            content_hash: id.to_string(),
            content: content.to_string(),
            metadata: ChunkMetadata {
                source_file: "doc.md".to_string(),
                source_path: "docs/doc.md".to_string(),
                heading: None,
                location: None,
                start_line: 1,
                end_line: 1,
            },
        }
    }

    fn retriever() -> HybridRetriever {
        let embedder = Box::new(HashingEmbedding::default());
        let mut retriever = HybridRetriever::new(embedder);

        for (id, text) in [
            (
                "a",
                "la indexación incremental solo procesa archivos modificados",
            ),
            ("b", "el motor de chat hace streaming token a token"),
            ("c", "configuración del tema claro y oscuro de la interfaz"),
        ] {
            let content = chunk(id, text);
            let embedding = HashingEmbedding::default().embed(&content.content).vector;
            retriever.add(content, Some(embedding));
        }

        retriever
    }

    #[test]
    fn recupera_el_fragmento_mas_relevante() {
        let retriever = retriever();
        let options = RetrieverOptions {
            top_k: 3,
            min_score: 0.1,
        };
        let results = retriever.retrieve("indexación incremental de archivos", &options);
        assert!(!results.is_empty());
        assert_eq!(results[0].chunk.id, "a");
        assert!(results[0].score >= results[results.len() - 1].score);
    }

    #[test]
    fn respeta_el_umbral_y_top_k() {
        let retriever = retriever();
        let strict = RetrieverOptions {
            top_k: 1,
            min_score: 0.95,
        };
        let results = retriever.retrieve("streaming token a token", &strict);
        assert!(results.len() <= 1);
    }

    #[test]
    fn sin_relacion_no_devuelve_resultados() {
        let retriever = retriever();
        let options = RetrieverOptions {
            top_k: 5,
            min_score: 0.8,
        };
        let results = retriever.retrieve("zeppelin fotosíntesis cuántica", &options);
        assert!(results.is_empty());
    }

    #[test]
    fn funciona_sin_embeddings_usando_bm25() {
        let embedder = Box::new(HashingEmbedding::default());
        let mut retriever = HybridRetriever::new(embedder);
        retriever.add(chunk("x", "recuperación léxica por bm25"), None);

        let options = RetrieverOptions {
            top_k: 3,
            min_score: 0.1,
        };
        let results = retriever.retrieve("bm25", &options);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk.id, "x");
        assert_eq!(retriever.embedded_len(), 0);
    }
}
