//! Configuración persistente de la aplicación (`config.toml`).
//!
//! Los valores están alineados con las HU de configuración: tema (HU-26),
//! parámetros del RAG (HU-27), temperatura bloqueada por defecto (HU-13) y
//! proveedor/modelo por defecto (HU-15).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// Temperatura por defecto, fijada en 0.1 para minimizar alucinaciones (HU-13).
pub const DEFAULT_TEMPERATURE: f32 = 0.1;

/// Tema visual de la interfaz (HU-26).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Tema claro.
    Light,
    /// Tema oscuro.
    Dark,
    /// Sigue la preferencia del sistema operativo.
    #[default]
    System,
}

/// Parámetros del motor RAG (HU-27).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RagConfig {
    /// Tamaño objetivo de un chunk, en caracteres.
    pub chunk_size: usize,
    /// Solapamiento entre chunks contiguos, en caracteres.
    pub chunk_overlap: usize,
    /// Número de fragmentos recuperados por consulta.
    pub top_k: usize,
    /// Umbral mínimo de similitud para descartar ruido.
    pub similarity_threshold: f32,
    /// Dimensionalidad de los embeddings locales.
    pub embedding_dimensions: usize,
}

impl Default for RagConfig {
    fn default() -> Self {
        Self {
            chunk_size: 800,
            chunk_overlap: 100,
            top_k: 5,
            similarity_threshold: 0.3,
            embedding_dimensions: 512,
        }
    }
}

/// Parámetros del modelo por defecto (HU-13, HU-15).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelConfig {
    /// Temperatura por defecto, bloqueada en 0.1 salvo modo avanzado.
    pub temperature: f32,
    /// Límite de tokens de salida.
    pub max_tokens: u32,
    /// Proveedor por defecto para sesiones nuevas.
    pub default_provider: String,
    /// Modelo por defecto para sesiones nuevas.
    pub default_model: String,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            temperature: DEFAULT_TEMPERATURE,
            max_tokens: 2048,
            default_provider: "deepseek".to_string(),
            default_model: "deepseek-chat".to_string(),
        }
    }
}

/// Configuración completa de la aplicación.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Tema visual.
    pub theme: Theme,
    /// Idioma de la interfaz (código ISO 639-1).
    pub language: String,
    /// Parámetros del RAG.
    pub rag: RagConfig,
    /// Parámetros del modelo.
    pub model: ModelConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            language: "es".to_string(),
            rag: RagConfig::default(),
            model: ModelConfig::default(),
        }
    }
}

impl AppConfig {
    /// Carga la configuración desde disco.
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path).map_err(|source| CoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let config: Self = toml::from_str(&raw)?;
        config.validate()?;
        Ok(config)
    }

    /// Carga la configuración y, ante cualquier error, registra un aviso y
    /// devuelve los valores por defecto en lugar de abortar el arranque.
    pub fn load_or_default(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        match Self::load(path) {
            Ok(config) => config,
            Err(err) => {
                tracing::warn!(
                    path = %path.display(),
                    "no se pudo cargar la configuración, se usan valores por defecto: {err}"
                );
                Self::default()
            }
        }
    }

    /// Guarda la configuración en disco, creando el directorio padre si falta.
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| CoreError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let serialized = toml::to_string_pretty(self)?;
        std::fs::write(path, serialized).map_err(|source| CoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(())
    }

    /// Comprueba que los valores estén dentro de rangos razonables.
    pub fn validate(&self) -> Result<()> {
        if !(0.0..=2.0).contains(&self.model.temperature) {
            return Err(CoreError::InvalidConfig(format!(
                "la temperatura debe estar entre 0.0 y 2.0 (valor: {})",
                self.model.temperature
            )));
        }
        if !(0.0..=1.0).contains(&self.rag.similarity_threshold) {
            return Err(CoreError::InvalidConfig(format!(
                "el umbral de similitud debe estar entre 0.0 y 1.0 (valor: {})",
                self.rag.similarity_threshold
            )));
        }
        if self.rag.chunk_size == 0 {
            return Err(CoreError::InvalidConfig(
                "el tamaño de chunk debe ser mayor que cero".to_string(),
            ));
        }
        if self.rag.chunk_overlap >= self.rag.chunk_size {
            return Err(CoreError::InvalidConfig(
                "el solapamiento debe ser menor que el tamaño de chunk".to_string(),
            ));
        }
        if self.rag.top_k == 0 {
            return Err(CoreError::InvalidConfig(
                "top-k debe ser al menos 1".to_string(),
            ));
        }
        if !(16..=8192).contains(&self.rag.embedding_dimensions) {
            return Err(CoreError::InvalidConfig(format!(
                "la dimensionalidad del embedding debe estar entre 16 y 8192 (valor: {})",
                self.rag.embedding_dimensions
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_valores_por_defecto_respetan_las_hu() {
        let config = AppConfig::default();
        assert_eq!(config.model.temperature, DEFAULT_TEMPERATURE);
        assert_eq!(config.rag.top_k, 5);
        assert_eq!(config.language, "es");
        config
            .validate()
            .expect("la configuración por defecto es válida");
    }

    #[test]
    fn se_serializa_y_recupera_sin_perdidas() {
        let original = AppConfig::default();
        let encoded = toml::to_string_pretty(&original).expect("serializa");
        let decoded: AppConfig = toml::from_str(&encoded).expect("deserializa");
        assert_eq!(original, decoded);
    }

    #[test]
    fn rechaza_solapamiento_mayor_que_el_chunk() {
        let config = AppConfig {
            rag: RagConfig {
                chunk_size: 100,
                chunk_overlap: 100,
                ..RagConfig::default()
            },
            ..AppConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn guarda_y_carga_desde_disco() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("raggy-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("config.toml");

        let config = AppConfig {
            theme: Theme::Dark,
            ..AppConfig::default()
        };
        config.save(&path).expect("guarda");
        let loaded = AppConfig::load(&path).expect("carga");
        assert_eq!(loaded.theme, Theme::Dark);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
