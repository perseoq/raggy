//! `localmind-rag` — motor de recuperación aumentada.
//!
//! La Fase 0 define los tipos y contratos del pipeline RAG: chunking semántico
//! (HU-09), generación de embeddings y recuperación por similitud (HU-10,
//! HU-11). Las implementaciones concretas llegan en las Fases 1 y 2.

pub mod chunk;
pub mod embedding;
pub mod retriever;

pub use chunk::{Chunk, ChunkMetadata, ChunkSource, Chunker, ChunkingOptions};
pub use embedding::{Embedding, EmbeddingProvider};
pub use retriever::{RetrievedChunk, Retriever, RetrieverOptions};
