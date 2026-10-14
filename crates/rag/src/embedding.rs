//! Contrato de generación de embeddings.

use serde::{Deserialize, Serialize};

/// Vector de embedding junto con su dimensionalidad implícita.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Embedding {
    /// Valores del vector.
    pub vector: Vec<f32>,
}

/// Origen del modelo de embeddings: local o remoto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingBackend {
    /// Modelo ejecutado localmente (candle/ort).
    Local,
    /// API de embeddings de un proveedor remoto.
    Remote,
}

/// Contrato del proveedor de embeddings.
///
/// Decidir entre [`EmbeddingBackend::Local`] y [`EmbeddingBackend::Remote`] es
/// una de las decisiones abiertas del proyecto y afecta a la promesa de
/// privacidad de HU-30.
pub trait EmbeddingProvider: Send + Sync {
    /// Nombre del modelo o backend.
    fn name(&self) -> &'static str;

    /// Dimensionalidad de los vectores producidos.
    fn dimensions(&self) -> usize;

    /// Backend utilizado.
    fn backend(&self) -> EmbeddingBackend;
}
