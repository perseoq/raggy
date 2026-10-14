//! `raggy-rag` — motor de recuperación aumentada.
//!
//! La Fase 1 implementa el **chunking semántico de Markdown** (HU-09): divide
//! cada archivo `.md` respetando encabezados, listas y bloques de código, y
//! anota cada fragmento con metadatos de origen (archivo, líneas, encabezado
//! padre) para poder citarlos después (HU-11).
//!
//! Los embeddings y la recuperación (HU-10) llegan en la Fase 2.

pub mod chunk;
pub mod embedding;
pub mod markdown;
pub mod retriever;

pub use chunk::{Chunk, ChunkMetadata, ChunkSource, Chunker, ChunkingOptions};
pub use embedding::{Embedding, EmbeddingBackend, EmbeddingProvider};
pub use markdown::MarkdownChunker;
pub use retriever::{RetrievedChunk, Retriever, RetrieverOptions};
