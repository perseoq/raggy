//! `raggy-rag` — motor de recuperación aumentada.
//!
//! La Fase 1 implementa el chunking semántico de Markdown (HU-09). La Fase 2
//! añade el motor de recuperación (HU-10): embeddings locales deterministas,
//! un índice vectorial por similitud coseno, un índice léxico BM25 y un
//! recuperador híbrido que fusiona ambos y aplica el umbral de similitud.
//!
//! Todo el cómputo ocurre en local y sin descargar modelos, en línea con la
//! promesa de privacidad de HU-30.

pub mod bm25;
pub mod chunk;
pub mod embedding;
pub mod markdown;
pub mod retriever;
pub mod text;
pub mod vector;

pub use bm25::Bm25Index;
pub use chunk::{Chunk, ChunkMetadata, ChunkSource, Chunker, ChunkingOptions};
pub use embedding::{
    cosine_similarity, Embedding, EmbeddingBackend, EmbeddingProvider, HashingEmbedding,
};
pub use markdown::MarkdownChunker;
pub use retriever::{Citation, HybridRetriever, RetrievedChunk, Retriever, RetrieverOptions};
pub use vector::VectorIndex;
