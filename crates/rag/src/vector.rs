//! Índice vectorial en memoria con búsqueda por similitud coseno.
//!
//! Para el tamaño de los corpus objetivos (documentación local) una búsqueda
//! exhaustiva es suficiente y evita dependencias pesadas o servicios externos.

use crate::embedding::cosine_similarity;

/// Índice de vectores asociados a identificadores de fragmento.
pub struct VectorIndex {
    dimensions: usize,
    entries: Vec<(String, Vec<f32>)>,
}

impl VectorIndex {
    /// Crea un índice para vectores de la dimensionalidad indicada.
    pub fn new(dimensions: usize) -> Self {
        Self {
            dimensions,
            entries: Vec::new(),
        }
    }

    /// Añade un vector; se ignora si su dimensionalidad no coincide.
    pub fn add(&mut self, id: impl Into<String>, vector: Vec<f32>) {
        if vector.len() == self.dimensions {
            self.entries.push((id.into(), vector));
        }
    }

    /// Número de vectores indexados.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// ¿Está vacío el índice?
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Devuelve los `top_k` vectores con similitud mayor o igual que `min_score`.
    pub fn search(&self, query: &[f32], top_k: usize, min_score: f32) -> Vec<(String, f32)> {
        let mut scored: Vec<(String, f32)> = self
            .entries
            .iter()
            .map(|(id, vector)| (id.clone(), cosine_similarity(query, vector)))
            .filter(|(_, score)| *score >= min_score)
            .collect();

        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored.truncate(top_k);
        scored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordena_por_similitud_y_respeta_el_umbral() {
        let mut index = VectorIndex::new(3);
        index.add("a", vec![1.0, 0.0, 0.0]);
        index.add("b", vec![0.9, 0.1, 0.0]);
        index.add("c", vec![0.0, 0.0, 1.0]);

        let hits = index.search(&[1.0, 0.0, 0.0], 3, 0.5);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].0, "a");
        assert!(hits[0].1 >= hits[1].1);
    }

    #[test]
    fn ignora_vectores_de_dimensionalidad_incorrecta() {
        let mut index = VectorIndex::new(3);
        index.add("malo", vec![1.0, 0.0]);
        assert!(index.is_empty());
    }
}
