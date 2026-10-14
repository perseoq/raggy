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
