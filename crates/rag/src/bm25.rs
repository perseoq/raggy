//! Índice BM25 para la recuperación léxica.

use std::collections::HashMap;

use crate::text::tokenize;

/// Parámetro de saturación de la frecuencia de término.
const K1: f32 = 1.2;

/// Parámetro de normalización por longitud del documento.
const B: f32 = 0.75;

struct Bm25Document {
    id: String,
    frequencies: HashMap<String, u32>,
    length: usize,
}

/// Índice BM25 construido incrementando documentos.
#[derive(Default)]
pub struct Bm25Index {
    documents: Vec<Bm25Document>,
    document_frequency: HashMap<String, usize>,
    total_length: usize,
}

impl Bm25Index {
    /// Crea un índice vacío.
    pub fn new() -> Self {
        Self::default()
    }

    /// Añade un documento.
    pub fn add(&mut self, id: impl Into<String>, text: &str) {
        let tokens = tokenize(text);
        let mut frequencies: HashMap<String, u32> = HashMap::new();
        for token in &tokens {
            *frequencies.entry(token.clone()).or_insert(0) += 1;
        }
        for term in frequencies.keys() {
            *self.document_frequency.entry(term.clone()).or_insert(0) += 1;
        }

        self.total_length += tokens.len();
        self.documents.push(Bm25Document {
            id: id.into(),
            frequencies,
            length: tokens.len(),
        });
    }

    /// Número de documentos indexados.
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    /// ¿Está vacío el índice?
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    fn average_length(&self) -> f32 {
        if self.documents.is_empty() {
            0.0
        } else {
            self.total_length as f32 / self.documents.len() as f32
        }
    }

    /// Devuelve los `top_k` documentos con mejor puntuación, de mayor a menor.
    pub fn search(&self, query: &str, top_k: usize) -> Vec<(String, f32)> {
        let terms = tokenize(query);
        if terms.is_empty() || self.documents.is_empty() {
            return Vec::new();
        }

        let total = self.documents.len() as f32;
        let average = self.average_length().max(f32::EPSILON);
        let mut scored = Vec::new();

        for document in &self.documents {
            let mut score = 0.0;
            for term in &terms {
                let Some(frequency) = document.frequencies.get(term).copied() else {
                    continue;
                };
                let document_frequency =
                    self.document_frequency.get(term).copied().unwrap_or(0) as f32;
                let idf =
                    (1.0 + (total - document_frequency + 0.5) / (document_frequency + 0.5)).ln();
                let frequency = frequency as f32;
                let normalization = 1.0 - B + B * (document.length as f32 / average);
                score += idf * (frequency * (K1 + 1.0)) / (frequency + K1 * normalization);
            }

            if score > 0.0 {
                scored.push((document.id.clone(), score));
            }
        }

        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored.truncate(top_k);
        scored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puntua_mas_los_documentos_relevantes() {
        let mut index = Bm25Index::new();
        index.add("doc-uno", "la indexación recursiva de carpetas markdown");
        index.add("doc-dos", "receta de tortilla de patatas");

        let hits = index.search("indexación carpetas", 5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "doc-uno");
    }

    #[test]
    fn sin_coincidencias_no_devuelve_nada() {
        let mut index = Bm25Index::new();
        index.add("doc", "contenido irrelevante");
        assert!(index.search("zzzz", 5).is_empty());
    }
}
