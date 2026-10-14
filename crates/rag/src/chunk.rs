//! Modelo de fragmentos (chunks) y contrato del chunker (HU-09).

use serde::{Deserialize, Serialize};

/// Metadatos de trazabilidad de un fragmento (HU-11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkMetadata {
    /// Nombre del archivo de origen.
    pub source_file: String,
    /// Ruta relativa a la carpeta indexada.
    pub source_path: String,
    /// Encabezado padre más cercano, si lo hay.
    pub heading: Option<String>,
    /// Primera línea incluida (1-based).
    pub start_line: usize,
    /// Última línea incluida (1-based).
    pub end_line: usize,
}

/// Fragmento de documento listo para indexar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    /// Identificador estable derivado del contenido y la ubicación.
    pub id: String,
    /// Hash del contenido del fragmento, útil para detectar cambios.
    pub content_hash: String,
    /// Texto del fragmento.
    pub content: String,
    /// Metadatos de origen.
    pub metadata: ChunkMetadata,
}

/// Documento de entrada para el chunker.
#[derive(Debug, Clone, Copy)]
pub struct ChunkSource<'a> {
    /// Ruta relativa mostrada al usuario.
    pub relative_path: &'a str,
    /// Contenido completo del archivo Markdown.
    pub content: &'a str,
}

/// Opciones de chunking derivadas de [`crate::ChunkingOptions`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkingOptions {
    /// Tamaño objetivo de un fragmento, en caracteres.
    pub max_chars: usize,
    /// Solapamiento entre fragmentos contiguos, en caracteres.
    pub overlap_chars: usize,
}

impl Default for ChunkingOptions {
    fn default() -> Self {
        Self {
            max_chars: 800,
            overlap_chars: 100,
        }
    }
}

/// Contrato del chunker semántico de Markdown.
pub trait Chunker: Send + Sync {
    /// Nombre del algoritmo, útil para diagnósticos.
    fn name(&self) -> &'static str;

    /// Divide un documento en fragmentos coherentes.
    fn chunk(&self, source: &ChunkSource<'_>, options: &ChunkingOptions) -> Vec<Chunk>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn las_opciones_por_defecto_son_sensatas() {
        let options = ChunkingOptions::default();
        assert!(options.overlap_chars < options.max_chars);
    }
}
