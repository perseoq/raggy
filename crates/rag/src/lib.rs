//! `raggy-rag` — motor de recuperación aumentada.
//!
//! Incluye el chunking semántico de Markdown (encabezados, listas y bloques de
//! código) y el motor de recuperación: embeddings locales deterministas, un
//! índice vectorial por similitud coseno, un índice léxico BM25 y un recuperador
//! híbrido que fusiona ambos y aplica el umbral de similitud.
//!
//! Todo el cómputo ocurre en local y sin descargar modelos, para no enviar el
//! corpus a terceros.

pub mod bm25;
pub mod chunk;
pub mod embedding;
pub mod markdown;
pub mod retriever;
pub mod section;
pub mod text;
pub mod vector;

pub use bm25::{Bm25Index, Bm25Parameters, DEFAULT_BM25_B, DEFAULT_BM25_K1};
pub use chunk::{
    Chunk, ChunkMetadata, ChunkSource, Chunker, ChunkingOptions, DEFAULT_CHUNK_OVERLAP,
    DEFAULT_CHUNK_SIZE,
};
pub use embedding::{
    cosine_similarity, Embedding, EmbeddingBackend, EmbeddingProvider, HashingEmbedding,
    DEFAULT_EMBEDDING_DIMENSIONS, MIN_EMBEDDING_DIMENSIONS,
};
pub use markdown::MarkdownChunker;
pub use retriever::{
    verify_citations, Citation, HybridRetriever, HybridRetrieverOptions, RetrievedChunk, Retriever,
    RetrieverOptions, DEFAULT_CANDIDATE_MULTIPLIER, DEFAULT_RELATIVE_FLOOR, DEFAULT_TOP_K,
    DEFAULT_VECTOR_WEIGHT, MIN_ABSOLUTE_SCORE, MIN_CANDIDATE_POOL,
};
pub use section::{chunk_sections, DocumentSection, SectionLocation};
pub use vector::VectorIndex;
