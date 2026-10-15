//! Recuperación de contexto: fusión híbrida de similitud vectorial y BM25.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::bm25::{Bm25Index, Bm25Parameters, DEFAULT_BM25_B, DEFAULT_BM25_K1};
use crate::chunk::Chunk;
use crate::embedding::EmbeddingProvider;
use crate::vector::VectorIndex;

/// Valores estándar de recuperación para consultas nuevas.
pub const DEFAULT_TOP_K: usize = 5;

/// Fracción del mejor resultado que debe alcanzar una coincidencia para entrar
/// en el contexto (0.0 = todo, 1.0 = solo el mejor). Un corte relativo evita
/// descartar la respuesta correcta cuando la escala absoluta de la consulta es
/// baja, algo habitual con los embeddings locales.
pub const DEFAULT_RELATIVE_FLOOR: f32 = 0.3;

/// Puntuación absoluta mínima para considerar que existe alguna señal. Solo
/// descarta resultados con coincidencia prácticamente nula; no se usa para
/// juzgar relevancia, porque las consultas sin relación pueden puntuar en un
/// rango parecido al de las relacionadas.
pub const MIN_ABSOLUTE_SCORE: f32 = 0.02;

/// Cita verificable de un fragmento (HU-11).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Citation {
    /// Nombre del archivo de origen.
    pub source_file: String,
    /// Ruta relativa a la carpeta indexada.
    pub source_path: String,
    /// Encabezado padre, si lo hay.
    pub heading: Option<String>,
    /// Ubicación legible en el documento original (p. ej. `pág. 3`), si aplica.
    #[serde(default)]
    pub location: Option<String>,
    /// Primera línea citada (1-based).
    pub start_line: usize,
    /// Última línea citada (1-based).
    pub end_line: usize,
    /// Puntuación de recuperación.
    pub score: f32,
}

impl Citation {
    /// Construye la cita de un fragmento.
    pub fn from_chunk(chunk: &Chunk, score: f32) -> Self {
        Self {
            source_file: chunk.metadata.source_file.clone(),
            source_path: chunk.metadata.source_path.clone(),
            heading: chunk.metadata.heading.clone(),
            location: chunk.metadata.location.clone(),
            start_line: chunk.metadata.start_line,
            end_line: chunk.metadata.end_line,
            score,
        }
    }

    /// Etiqueta canónica, p. ej. `docs/guia.md (líneas 10-24)` o
    /// `datos.xlsx (hoja Ventas, A2:D20)`.
    pub fn label(&self) -> String {
        let location = self
            .location
            .as_deref()
            .filter(|location| !location.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("líneas {}-{}", self.start_line, self.end_line));
        format!("{} ({location})", self.source_path)
    }

    /// Comprueba que el texto cite literalmente esta fuente y no la mencione
    /// solo dentro de código generado.
    pub fn is_verified_in(&self, answer: &str) -> bool {
        let mut markers = vec![format!("[{}]", self.label())];
        if self
            .location
            .as_deref()
            .is_none_or(|location| location.starts_with("líneas "))
        {
            // Las conversaciones previas a HU-11 ya pueden contener el formato
            // `ruta:líneas`; se verifica solo si coincide con ese rango real.
            markers.push(format!(
                "[{}:{}-{}]",
                self.source_path, self.start_line, self.end_line
            ));
        }
        answer.split("```").step_by(2).any(|prose| {
            prose
                .split('`')
                .step_by(2)
                .any(|text| markers.iter().any(|marker| text.contains(marker)))
        })
    }

    /// Comprueba si esta cita apunta al mismo fragmento almacenado.
    pub fn matches_chunk(&self, chunk: &Chunk) -> bool {
        let metadata = &chunk.metadata;
        metadata.source_path == self.source_path
            && metadata.start_line == self.start_line
            && metadata.end_line == self.end_line
            && metadata.location == self.location
    }
}

/// Conserva solo las fuentes recuperadas que la respuesta cita literalmente.
/// Las etiquetas repetidas se devuelven una sola vez, en orden de recuperación.
pub fn verify_citations(answer: &str, candidates: &[Citation]) -> Vec<Citation> {
    let mut labels = HashSet::new();
    candidates
        .iter()
        .filter(|citation| citation.is_verified_in(answer))
        .filter(|citation| labels.insert(citation.label()))
        .cloned()
        .collect()
}

/// Fragmento recuperado junto con sus puntuaciones.
#[derive(Debug, Clone, PartialEq)]
pub struct RetrievedChunk {
    /// Fragmento original.
    pub chunk: Chunk,
    /// Puntuación combinada usada para ordenar (en `0.0..=1.0`).
    pub score: f32,
    /// Similitud vectorial normalizada a `0.0..=1.0`.
    pub vector_score: f32,
    /// Puntuación léxica (BM25) normalizada a `0.0..=1.0`.
    pub lexical_score: f32,
}

impl RetrievedChunk {
    /// Cita verificable del fragmento (HU-11).
    pub fn citation(&self) -> Citation {
        Citation::from_chunk(&self.chunk, self.score)
    }
}

/// Parámetros de recuperación.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetrieverOptions {
    /// Número máximo de fragmentos a devolver.
    pub top_k: usize,
    /// Corte **relativo** al mejor resultado: se conservan las coincidencias cuya
    /// puntuación combinada sea al menos esta fracción de la mejor (0.0..=1.0).
    pub relative_floor: f32,
}

impl Default for RetrieverOptions {
    fn default() -> Self {
        Self {
            top_k: DEFAULT_TOP_K,
            relative_floor: DEFAULT_RELATIVE_FLOOR,
        }
    }
}

/// Contrato del recuperador de contexto.
pub trait Retriever: Send + Sync {
    /// Recupera los fragmentos más relevantes para `query`.
    fn retrieve(&self, query: &str, options: &RetrieverOptions) -> Vec<RetrievedChunk>;
}

/// Peso de la componente vectorial en la fusión (el resto es BM25).
pub const DEFAULT_VECTOR_WEIGHT: f32 = 0.5;

/// Número de candidatos evaluados por cada resultado solicitado.
pub const DEFAULT_CANDIDATE_MULTIPLIER: usize = 4;
/// Amplitud mínima del pool híbrido para que una lista pequeña de `top_k` no
/// elimine candidatos antes de fusionar ambas señales.
pub const MIN_CANDIDATE_POOL: usize = 16;

/// Parámetros del recuperador híbrido, separados de los límites por consulta.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HybridRetrieverOptions {
    /// Peso vectorial en la puntuación combinada (`0.0..=1.0`).
    pub vector_weight: f32,
    /// Parámetros del índice BM25.
    pub bm25: Bm25Parameters,
    /// Sobrerrecuperación antes de fusionar resultados.
    pub candidate_multiplier: usize,
    /// Mínimo de candidatos antes de fusionar resultados.
    pub minimum_candidate_pool: usize,
}

impl Default for HybridRetrieverOptions {
    fn default() -> Self {
        Self {
            vector_weight: DEFAULT_VECTOR_WEIGHT,
            bm25: Bm25Parameters {
                k1: DEFAULT_BM25_K1,
                b: DEFAULT_BM25_B,
            },
            candidate_multiplier: DEFAULT_CANDIDATE_MULTIPLIER,
            minimum_candidate_pool: MIN_CANDIDATE_POOL,
        }
    }
}

/// Recuperador híbrido que combina embeddings locales y BM25.
pub struct HybridRetriever {
    embedder: Box<dyn EmbeddingProvider>,
    vector: VectorIndex,
    lexical: Bm25Index,
    chunks: HashMap<String, Chunk>,
    options: HybridRetrieverOptions,
}

impl HybridRetriever {
    /// Crea el recuperador a partir de un proveedor de embeddings.
    pub fn new(embedder: Box<dyn EmbeddingProvider>) -> Self {
        Self::with_options(embedder, HybridRetrieverOptions::default())
    }

    /// Crea el recuperador con pesos BM25/vectorial y amplitud de candidatos
    /// explícitos para ajustar la recuperación sin cambiar el chunker.
    pub fn with_options(
        embedder: Box<dyn EmbeddingProvider>,
        mut options: HybridRetrieverOptions,
    ) -> Self {
        options.vector_weight = if options.vector_weight.is_finite() {
            options.vector_weight.clamp(0.0, 1.0)
        } else {
            DEFAULT_VECTOR_WEIGHT
        };
        options.candidate_multiplier = options.candidate_multiplier.max(1);
        options.minimum_candidate_pool = options.minimum_candidate_pool.max(1);
        let dimensions = embedder.dimensions();
        Self {
            embedder,
            vector: VectorIndex::new(dimensions),
            lexical: Bm25Index::with_parameters(options.bm25),
            chunks: HashMap::new(),
            options,
        }
    }

    /// Añade un fragmento con su embedding (opcional).
    ///
    /// Si el embedding falta o su dimensionalidad no coincide con la del
    /// proveedor, el fragmento sigue siendo recuperable por BM25.
    pub fn add(&mut self, chunk: Chunk, embedding: Option<Vec<f32>>) {
        self.lexical.add(chunk.id.clone(), &chunk.content);
        if let Some(vector) = embedding {
            self.vector.add(chunk.id.clone(), vector);
        }
        self.chunks.insert(chunk.id.clone(), chunk);
    }

    /// Número de fragmentos indexados.
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// ¿Está vacío el recuperador?
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Número de fragmentos con embedding utilizable.
    pub fn embedded_len(&self) -> usize {
        self.vector.len()
    }
}

impl Retriever for HybridRetriever {
    fn retrieve(&self, query: &str, options: &RetrieverOptions) -> Vec<RetrievedChunk> {
        if self.chunks.is_empty() || query.trim().is_empty() {
            return Vec::new();
        }

        let pool = options
            .top_k
            .saturating_mul(self.options.candidate_multiplier)
            .max(self.options.minimum_candidate_pool);

        let query_embedding = self.embedder.embed(query).vector;
        let vector_hits = self.vector.search(&query_embedding, pool, f32::MIN);
        let lexical_hits = self.lexical.search(query, pool);

        let mut fused: HashMap<String, (f32, f32)> = HashMap::new();
        for (id, score) in vector_hits {
            fused.entry(id).or_insert((0.0, 0.0)).0 = score.clamp(0.0, 1.0);
        }
        for (id, score) in lexical_hits {
            fused.entry(id).or_insert((0.0, 0.0)).1 = score.clamp(0.0, 1.0);
        }

        let mut results: Vec<RetrievedChunk> = fused
            .into_iter()
            .filter_map(|(id, (vector_score, lexical_score))| {
                let chunk = self.chunks.get(&id)?.clone();
                let score = self.options.vector_weight * vector_score
                    + (1.0 - self.options.vector_weight) * lexical_score;
                Some(RetrievedChunk {
                    chunk,
                    score,
                    vector_score,
                    lexical_score,
                })
            })
            .filter(|result| result.score >= MIN_ABSOLUTE_SCORE)
            .collect();

        // El corte es relativo al mejor resultado: si hay alguna coincidencia,
        // nunca se descarta la mejor por un umbral absoluto mal calibrado.
        if let Some(best) = results.iter().map(|result| result.score).reduce(f32::max) {
            let floor = (best * options.relative_floor.clamp(0.0, 1.0)).max(MIN_ABSOLUTE_SCORE);
            results.retain(|result| result.score >= floor);
        }

        results.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.chunk.id.cmp(&b.chunk.id))
        });
        results.truncate(options.top_k);
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::ChunkMetadata;
    use crate::embedding::HashingEmbedding;

    fn chunk(id: &str, content: &str) -> Chunk {
        Chunk {
            id: id.to_string(),
            content_hash: id.to_string(),
            content: content.to_string(),
            metadata: ChunkMetadata {
                source_file: "doc.md".to_string(),
                source_path: "docs/doc.md".to_string(),
                heading: None,
                location: None,
                start_line: 1,
                end_line: 1,
            },
        }
    }

    #[test]
    fn etiqueta_canonica_prefiere_la_ubicacion_del_formato() {
        let mut source = chunk("doc", "texto");
        source.metadata.location = Some("página 3".to_string());
        let page = Citation::from_chunk(&source, 0.8);
        assert_eq!(page.label(), "docs/doc.md (página 3)");
        assert!(page.matches_chunk(&source));

        source.metadata.location = Some("hoja Ventas, A2:D4".to_string());
        let cells = Citation::from_chunk(&source, 0.8);
        assert_eq!(cells.label(), "docs/doc.md (hoja Ventas, A2:D4)");

        source.metadata.location = None;
        source.metadata.start_line = 10;
        source.metadata.end_line = 14;
        let lines = Citation::from_chunk(&source, 0.8);
        assert_eq!(lines.label(), "docs/doc.md (líneas 10-14)");
        assert!(lines.matches_chunk(&source));
    }

    #[test]
    fn solo_verifica_fuentes_recuperadas_citadas_fuera_de_codigo() {
        let first = Citation::from_chunk(&chunk("a", "uno"), 0.9);
        let mut second_chunk = chunk("b", "dos");
        second_chunk.metadata.source_path = "docs/other.md".to_string();
        let second = Citation::from_chunk(&second_chunk, 0.8);
        let answer = format!(
            "Dato comprobable [{}]. `código [{}]`\n```text\n[{}]\n```",
            first.label(),
            second.label(),
            second.label()
        );

        let verified = verify_citations(&answer, &[first.clone(), second.clone(), first.clone()]);
        assert_eq!(verified, vec![first.clone()]);
        assert!(!second.is_verified_in(&answer));
        assert!(verify_citations("[fuente-inventada]", &[second]).is_empty());
        assert!(first.is_verified_in("Dato heredado [docs/doc.md:1-1]."));
    }

    fn retriever() -> HybridRetriever {
        let embedder = Box::new(HashingEmbedding::default());
        let mut retriever = HybridRetriever::new(embedder);

        for (id, text) in [
            (
                "a",
                "la indexación incremental solo procesa archivos modificados",
            ),
            ("b", "el motor de chat hace streaming token a token"),
            ("c", "configuración del tema claro y oscuro de la interfaz"),
        ] {
            let content = chunk(id, text);
            let embedding = HashingEmbedding::default().embed(&content.content).vector;
            retriever.add(content, Some(embedding));
        }

        retriever
    }

    #[test]
    fn recupera_el_fragmento_mas_relevante() {
        let retriever = retriever();
        let options = RetrieverOptions {
            top_k: 3,
            relative_floor: 0.1,
        };
        let results = retriever.retrieve("indexación incremental de archivos", &options);
        assert!(!results.is_empty());
        assert_eq!(results[0].chunk.id, "a");
        assert!(results[0].score >= results[results.len() - 1].score);
    }

    #[test]
    fn respeta_el_corte_relativo_y_top_k() {
        let retriever = retriever();
        let strict = RetrieverOptions {
            top_k: 1,
            relative_floor: 0.95,
        };
        let results = retriever.retrieve("streaming token a token", &strict);
        assert!(results.len() <= 1);
    }

    #[test]
    fn sin_relacion_no_devuelve_resultados() {
        let retriever = retriever();
        let options = RetrieverOptions {
            top_k: 5,
            relative_floor: 0.8,
        };
        let results = retriever.retrieve("zeppelin fotosíntesis cuántica", &options);
        assert!(results.is_empty());
    }

    #[test]
    fn el_corte_relativo_no_descarta_la_mejor_coincidencia() {
        // Fragmento cuya puntuación combinada es baja en términos absolutos
        // (~0.2): con un umbral absoluto de 0.3 se perdería, pero al ser el
        // mejor resultado debe conservarse.
        let mut retriever = retriever();
        retriever.add(
            chunk(
                "d",
                "cada persona dispone de 23 dias laborables de descanso al ano",
            ),
            None,
        );

        let options = RetrieverOptions::default();
        let results = retriever.retrieve("¿cuántos días de descanso tengo al año?", &options);

        assert!(!results.is_empty(), "debe devolver el mejor candidato");
        assert_eq!(results[0].chunk.id, "d");
        assert!(
            results[0].score < 0.3,
            "el caso reproduce una puntuación baja: {}",
            results[0].score
        );
    }

    #[test]
    fn funciona_sin_embeddings_usando_bm25() {
        let embedder = Box::new(HashingEmbedding::default());
        let mut retriever = HybridRetriever::new(embedder);
        retriever.add(chunk("x", "recuperación léxica por bm25"), None);

        let options = RetrieverOptions {
            top_k: 3,
            relative_floor: 0.1,
        };
        let results = retriever.retrieve("bm25", &options);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk.id, "x");
        assert_eq!(retriever.embedded_len(), 0);
    }
}
