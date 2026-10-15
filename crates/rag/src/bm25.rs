//! Índice BM25 para la recuperación léxica.

use std::collections::{BTreeSet, HashMap};

use crate::text::tokenize;

/// Valores BM25 estándar usados si no se especifica otra parametrización.
pub const DEFAULT_BM25_K1: f32 = 1.2;
pub const DEFAULT_BM25_B: f32 = 0.75;

/// Parámetros del índice léxico; `b` controla cuánto pesa la longitud del texto.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bm25Parameters {
    /// Saturación de la frecuencia del término; debe ser mayor que cero.
    pub k1: f32,
    /// Normalización por longitud del documento, entre cero y uno.
    pub b: f32,
}

impl Default for Bm25Parameters {
    fn default() -> Self {
        Self {
            k1: DEFAULT_BM25_K1,
            b: DEFAULT_BM25_B,
        }
    }
}

struct Bm25Document {
    id: String,
    frequencies: HashMap<String, u32>,
    length: usize,
}

/// Índice BM25 construido incrementando documentos.
pub struct Bm25Index {
    documents: Vec<Bm25Document>,
    document_frequency: HashMap<String, usize>,
    total_length: usize,
    parameters: Bm25Parameters,
}

impl Default for Bm25Index {
    fn default() -> Self {
        Self::new()
    }
}

impl Bm25Index {
    /// Crea un índice vacío.
    pub fn new() -> Self {
        Self::with_parameters(Bm25Parameters::default())
    }

    /// Crea un índice con parámetros léxicos explícitos.
    pub fn with_parameters(parameters: Bm25Parameters) -> Self {
        let parameters = Bm25Parameters {
            k1: if parameters.k1.is_finite() {
                parameters.k1.max(f32::EPSILON)
            } else {
                DEFAULT_BM25_K1
            },
            b: if parameters.b.is_finite() {
                parameters.b.clamp(0.0, 1.0)
            } else {
                DEFAULT_BM25_B
            },
        };
        Self {
            documents: Vec::new(),
            document_frequency: HashMap::new(),
            total_length: 0,
            parameters,
        }
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
        let terms: Vec<String> = tokenize(query)
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if terms.is_empty() || self.documents.is_empty() {
            return Vec::new();
        }

        let total = self.documents.len() as f32;
        let average = self.average_length().max(f32::EPSILON);
        let query_length = terms.len() as f32;
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
                let normalization = 1.0 - self.parameters.b
                    + self.parameters.b * (document.length as f32 / average);
                score += idf * (frequency * (self.parameters.k1 + 1.0))
                    / (frequency + self.parameters.k1 * normalization);
            }

            if score > 0.0 {
                // Normaliza contra la cantidad de términos distintos de la
                // consulta, no contra el mejor hit: un término ubicuo no debe
                // convertirse automáticamente en una relevancia de 1.0.
                scored.push((document.id.clone(), score / (score + query_length)));
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

    #[test]
    fn normaliza_bm25_sin_forzar_a_uno_el_mejor_hit() {
        let mut index = Bm25Index::new();
        index.add("comun-a", "comun comun");
        index.add("comun-b", "comun");
        index.add("raro", "extraordinario");

        let common = index.search("comun", 5);
        let rare = index.search("extraordinario", 5);
        assert!(common[0].1 < 1.0);
        assert!(rare[0].1 > common[0].1);
        assert!(rare[0].1 <= 1.0);
        assert_eq!(index.search("extraordinario extraordinario", 5), rare);
    }
}
