//! Contrato de generación de embeddings y backend local por *feature hashing*.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::text::{fnv1a, tokenize};

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
    /// Modelo ejecutado localmente.
    Local,
    /// API de embeddings de un proveedor remoto.
    Remote,
}

/// Contrato del proveedor de embeddings.
pub trait EmbeddingProvider: Send + Sync {
    /// Nombre del modelo o backend.
    fn name(&self) -> &'static str;

    /// Dimensionalidad de los vectores producidos.
    fn dimensions(&self) -> usize;

    /// Backend utilizado.
    fn backend(&self) -> EmbeddingBackend;

    /// Calcula el embedding de un texto.
    fn embed(&self, text: &str) -> Embedding;
}

/// Peso de los bigramas frente a los unigramas.
const BIGRAM_WEIGHT: f32 = 0.5;

/// Sal que distingue los bigramas de los unigramas.
const BIGRAM_SALT: u64 = 0x9e37_79b9_7f4a_7c15;

/// Embedding local determinista basado en *feature hashing*.
///
/// No requiere descargar ningún modelo y funciona sin red, en línea con la
/// promesa de privacidad de HU-30. Aproxima la similitud léxica: útil para
/// documentación técnica, y sustituible por un modelo neuronal implementando
/// el trait [`EmbeddingProvider`].
#[derive(Debug, Clone)]
pub struct HashingEmbedding {
    dimensions: usize,
}

impl HashingEmbedding {
    /// Crea el backend con la dimensionalidad indicada.
    pub fn new(dimensions: usize) -> Self {
        Self {
            dimensions: dimensions.max(16),
        }
    }
}

impl Default for HashingEmbedding {
    fn default() -> Self {
        Self::new(512)
    }
}

impl EmbeddingProvider for HashingEmbedding {
    fn name(&self) -> &'static str {
        "hashing-tf"
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn backend(&self) -> EmbeddingBackend {
        EmbeddingBackend::Local
    }

    fn embed(&self, text: &str) -> Embedding {
        let tokens = tokenize(text);
        let mut vector = vec![0.0f32; self.dimensions];
        if tokens.is_empty() {
            return Embedding { vector };
        }

        let mut unigrams: HashMap<u64, f32> = HashMap::new();
        for token in &tokens {
            *unigrams.entry(fnv1a(token)).or_insert(0.0) += 1.0;
        }

        let mut bigrams: HashMap<u64, f32> = HashMap::new();
        for pair in tokens.windows(2) {
            let key = format!("{}#{}", pair[0], pair[1]);
            *bigrams.entry(fnv1a(&key) ^ BIGRAM_SALT).or_insert(0.0) += 1.0;
        }

        accumulate(&mut vector, &unigrams, self.dimensions, 1.0);
        accumulate(&mut vector, &bigrams, self.dimensions, BIGRAM_WEIGHT);
        l2_normalize(&mut vector);

        Embedding { vector }
    }
}

/// Suma las características al vector con frecuencia sublineal.
fn accumulate(vector: &mut [f32], counts: &HashMap<u64, f32>, dimensions: usize, scale: f32) {
    let mut entries: Vec<(u64, f32)> = counts.iter().map(|(hash, count)| (*hash, *count)).collect();
    // Orden estable para que la suma en coma flotante sea determinista.
    entries.sort_unstable_by_key(|(hash, _)| *hash);

    for (hash, count) in entries {
        let index = (hash % dimensions as u64) as usize;
        let sign = if (hash >> 63) & 1 == 1 { -1.0 } else { 1.0 };
        vector[index] += sign * scale * (1.0 + count.ln());
    }
}

/// Normaliza el vector a norma unidad (en su lugar).
pub fn l2_normalize(vector: &mut [f32]) {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in vector.iter_mut() {
            *value /= norm;
        }
    }
}

/// Similitud coseno entre dos vectores de la misma longitud.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_vector_es_determinista_y_unitario() {
        let embedder = HashingEmbedding::new(256);
        let first = embedder.embed("indexación incremental de carpetas");
        let second = embedder.embed("indexación incremental de carpetas");
        assert_eq!(first, second);

        let norm = first.vector.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "norma = {norm}");
    }

    #[test]
    fn textos_similares_puntuan_mas_que_textos_distintos() {
        let embedder = HashingEmbedding::default();
        let base = embedder.embed("indexar carpetas de markdown");
        let similar = embedder.embed("indexar carpetas de markdown locales");
        let distinto = embedder.embed("receta de tortilla de patatas");

        let cerca = cosine_similarity(&base.vector, &similar.vector);
        let lejos = cosine_similarity(&base.vector, &distinto.vector);
        assert!(cerca > lejos, "cerca = {cerca}, lejos = {lejos}");
    }

    #[test]
    fn el_texto_vacio_produce_vector_nulo() {
        let embedder = HashingEmbedding::default();
        let embedding = embedder.embed("   ");
        assert!(embedding.vector.iter().all(|value| *value == 0.0));
    }
}
