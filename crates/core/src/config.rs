//! Configuración persistente de la aplicación (`settings.json`) y límites de ingesta.
//!
//! Los valores están alineados con las HU de configuración: tema (HU-26),
//! parámetros del RAG (HU-27), temperatura bloqueada por defecto (HU-13) y
//! proveedor/modelo por defecto (HU-15).

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::error::{CoreError, Result};

/// Temperatura por defecto, fijada en 0.1 para minimizar alucinaciones (HU-13).
pub const DEFAULT_TEMPERATURE: f32 = 0.1;

/// Límites de los controles de recuperación mostrados en Configuración.
pub const MIN_CHUNK_SIZE: usize = 100;
pub const MAX_CHUNK_SIZE: usize = 4_000;
pub const MAX_CHUNK_OVERLAP: usize = 2_000;
pub const MAX_TOP_K: usize = 50;
pub const MIN_SIMILARITY_THRESHOLD: f32 = 0.0;
pub const MAX_SIMILARITY_THRESHOLD: f32 = 1.0;
pub const MIN_EMBEDDING_DIMENSIONS: usize = raggy_rag::MIN_EMBEDDING_DIMENSIONS;
pub const MAX_EMBEDDING_DIMENSIONS: usize = 8_192;

const CHUNK_SIZE_WARNING_MIN: usize = 200;
const CHUNK_SIZE_WARNING_MAX: usize = 3_000;
const TOP_K_WARNING_MAX: usize = 20;
const SIMILARITY_WARNING_MIN: f32 = 0.1;
const SIMILARITY_WARNING_MAX: f32 = 0.8;
const EMBEDDING_WARNING_MIN: usize = 64;

/// Versión del aviso de privacidad que debe aceptar el usuario.
pub const PRIVACY_NOTICE_VERSION: u32 = 2;

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
            chunk_size: raggy_rag::DEFAULT_CHUNK_SIZE,
            chunk_overlap: raggy_rag::DEFAULT_CHUNK_OVERLAP,
            top_k: raggy_rag::DEFAULT_TOP_K,
            similarity_threshold: raggy_rag::DEFAULT_MIN_SCORE,
            embedding_dimensions: raggy_rag::DEFAULT_EMBEDDING_DIMENSIONS,
        }
    }
}

impl RagConfig {
    /// Avisos sobre combinaciones de valores poco recomendables (HU-27).
    pub fn warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.chunk_size < CHUNK_SIZE_WARNING_MIN {
            warnings.push(
                "El tamaño de fragmento es muy pequeño y puede dividir en exceso.".to_string(),
            );
        }
        if self.chunk_size > CHUNK_SIZE_WARNING_MAX {
            warnings.push(
                "El tamaño de fragmento es muy grande y puede diluir la relevancia.".to_string(),
            );
        }
        if self.top_k > TOP_K_WARNING_MAX {
            warnings
                .push("Recuperar más de 20 fragmentos suele degradar la precisión.".to_string());
        }
        if self.similarity_threshold < SIMILARITY_WARNING_MIN {
            warnings.push("Un umbral muy bajo dejará pasar contexto poco relevante.".to_string());
        }
        if self.similarity_threshold > SIMILARITY_WARNING_MAX {
            warnings.push("Un umbral muy alto puede descartar contexto útil.".to_string());
        }
        if self.embedding_dimensions < EMBEDDING_WARNING_MIN {
            warnings.push(
                "Una dimensionalidad de embedding muy baja reduce la calidad semántica."
                    .to_string(),
            );
        }

        warnings
    }
}

/// Parámetros de ingesta de documentos y OCR (fase 2.0.0).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IngestConfig {
    /// ¿Se aplica OCR a las páginas escaneadas si hay Tesseract?
    pub ocr_enabled: bool,
    /// Idiomas de OCR (códigos de Tesseract, p. ej. `spa` o `spa+eng`).
    pub ocr_languages: Vec<String>,
    /// Texto mínimo de una página para no considerarla escaneada.
    pub min_text_chars: usize,
    /// Tamaño máximo de archivo de entrada, en MiB.
    pub max_file_mb: u64,
    /// Máximo de bytes descomprimidos al leer un paquete Office (MiB).
    pub max_archive_mb: u64,
    /// Máximo de páginas PDF procesadas por documento.
    pub max_pages: usize,
    /// Máximo de texto extraído por página PDF, en MiB.
    pub max_pdf_page_text_mb: u64,
    /// Máximo de texto extraído por documento PDF, en MiB.
    pub max_pdf_text_mb: u64,
    /// Tiempo máximo de espera para cada herramienta externa, en segundos.
    pub process_timeout_secs: u64,
    /// Máximo aproximado de píxeles por página al rasterizar para OCR.
    pub max_ocr_image_pixels: u64,
}

impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            ocr_enabled: true,
            ocr_languages: vec!["spa".to_string()],
            min_text_chars: raggy_docs::limits::DEFAULT_MIN_TEXT_CHARS,
            max_file_mb: raggy_docs::limits::DEFAULT_MAX_FILE_MIB,
            max_archive_mb: raggy_docs::limits::DEFAULT_MAX_ARCHIVE_MIB,
            max_pages: raggy_docs::limits::DEFAULT_MAX_PAGES,
            max_pdf_page_text_mb: raggy_docs::limits::DEFAULT_MAX_PDF_PAGE_TEXT_MIB,
            max_pdf_text_mb: raggy_docs::limits::DEFAULT_MAX_PDF_TEXT_MIB,
            process_timeout_secs: raggy_docs::limits::DEFAULT_PROCESS_TIMEOUT_SECS,
            max_ocr_image_pixels: raggy_docs::limits::DEFAULT_MAX_OCR_IMAGE_PIXELS,
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
    /// Parámetros de ingesta de documentos y OCR.
    pub ingest: IngestConfig,
    /// Versión del aviso de privacidad que el usuario ya aceptó (HU-30).
    pub privacy_notice_version: u32,
    /// Último tamaño de la ventana en píxeles lógicos, para restaurarlo.
    pub window_size: Option<[f32; 2]>,
    /// Directorio de datos alternativo; si falta, se usa la carpeta base.
    pub data_dir: Option<PathBuf>,
    /// Claves de API por proveedor (clave del proveedor → token).
    pub api_keys: HashMap<String, String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            language: "es".to_string(),
            rag: RagConfig::default(),
            model: ModelConfig::default(),
            ingest: IngestConfig::default(),
            privacy_notice_version: 0,
            window_size: None,
            data_dir: None,
            api_keys: HashMap::new(),
        }
    }
}

impl AppConfig {
    /// Carga la configuración desde `settings.json`.
    pub fn load(path: &Path) -> Result<Self> {
        let mut raw = std::fs::read_to_string(path).map_err(|source| CoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let parsed = serde_json::from_str(&raw);
        raw.zeroize();
        let config: Self = parsed?;
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

    /// Guarda la configuración en `settings.json`, creando el directorio padre
    /// si falta y restringiendo los permisos del archivo.
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| CoreError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let mut document = serde_json::to_value(self)?;

        // Las claves se mantienen en un almacén separado. Al guardar cualquier
        // otra preferencia, conserva el campo más reciente del disco en vez de
        // sobrescribirlo con la copia potencialmente antigua de AppConfig.
        if path.is_file() {
            if let Ok(mut raw) = std::fs::read_to_string(path) {
                let existing = serde_json::from_str::<serde_json::Value>(&raw).ok();
                raw.zeroize();
                zeroize_api_keys(&mut document);
                match existing {
                    Some(serde_json::Value::Object(mut map)) => {
                        if let Some(api_keys) = map.remove("api_keys") {
                            if let serde_json::Value::Object(output) = &mut document {
                                output.insert("api_keys".to_string(), api_keys);
                            }
                        } else if let serde_json::Value::Object(output) = &mut document {
                            output.remove("api_keys");
                        }
                        zeroize_api_keys(&mut serde_json::Value::Object(map));
                    }
                    _ => {
                        if let serde_json::Value::Object(output) = &mut document {
                            output.remove("api_keys");
                        }
                    }
                }
            }
        }

        let mut serialized = serde_json::to_string_pretty(&document)?;
        zeroize_api_keys(&mut document);
        let write_result =
            write_settings_atomic(path, serialized.as_bytes()).map_err(|source| CoreError::Io {
                path: path.to_path_buf(),
                source,
            });
        serialized.zeroize();
        write_result?;
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
        if !(MIN_SIMILARITY_THRESHOLD..=MAX_SIMILARITY_THRESHOLD)
            .contains(&self.rag.similarity_threshold)
        {
            return Err(CoreError::InvalidConfig(format!(
                "el umbral de similitud debe estar entre {MIN_SIMILARITY_THRESHOLD} y {MAX_SIMILARITY_THRESHOLD} (valor: {})",
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
        if !(MIN_EMBEDDING_DIMENSIONS..=MAX_EMBEDDING_DIMENSIONS)
            .contains(&self.rag.embedding_dimensions)
        {
            return Err(CoreError::InvalidConfig(format!(
                "la dimensionalidad del embedding debe estar entre {MIN_EMBEDDING_DIMENSIONS} y {MAX_EMBEDDING_DIMENSIONS} (valor: {})",
                self.rag.embedding_dimensions
            )));
        }
        if self.ingest.ocr_enabled
            && self
                .ingest
                .ocr_languages
                .iter()
                .all(|language| language.trim().is_empty())
        {
            return Err(CoreError::InvalidConfig(
                "el OCR está activado pero no hay idiomas configurados".to_string(),
            ));
        }
        if self.ingest.max_file_mb == 0
            || self.ingest.max_file_mb > raggy_docs::limits::MAX_MAX_FILE_MIB
        {
            return Err(CoreError::InvalidConfig(format!(
                "el límite de tamaño de archivo debe estar entre 1 y {} MiB",
                raggy_docs::limits::MAX_MAX_FILE_MIB
            )));
        }
        if self.ingest.max_archive_mb == 0
            || self.ingest.max_archive_mb > raggy_docs::limits::MAX_MAX_ARCHIVE_MIB
        {
            return Err(CoreError::InvalidConfig(format!(
                "el límite de descompresión debe estar entre 1 y {} MiB",
                raggy_docs::limits::MAX_MAX_ARCHIVE_MIB
            )));
        }
        if self.ingest.max_pages == 0 || self.ingest.max_pages > raggy_docs::limits::MAX_MAX_PAGES {
            return Err(CoreError::InvalidConfig(format!(
                "el máximo de páginas debe estar entre 1 y {}",
                raggy_docs::limits::MAX_MAX_PAGES
            )));
        }
        if self.ingest.max_pdf_page_text_mb == 0
            || self.ingest.max_pdf_page_text_mb > raggy_docs::limits::MAX_MAX_PDF_PAGE_TEXT_MIB
        {
            return Err(CoreError::InvalidConfig(format!(
                "el límite de texto por página PDF debe estar entre 1 y {} MiB",
                raggy_docs::limits::MAX_MAX_PDF_PAGE_TEXT_MIB
            )));
        }
        if self.ingest.max_pdf_text_mb == 0
            || self.ingest.max_pdf_text_mb > raggy_docs::limits::MAX_MAX_PDF_TEXT_MIB
        {
            return Err(CoreError::InvalidConfig(format!(
                "el límite de texto PDF debe estar entre 1 y {} MiB",
                raggy_docs::limits::MAX_MAX_PDF_TEXT_MIB
            )));
        }
        if self.ingest.max_pdf_page_text_mb > self.ingest.max_pdf_text_mb {
            return Err(CoreError::InvalidConfig(
                "el límite de texto por página PDF no puede superar el del documento".to_string(),
            ));
        }
        if self.ingest.process_timeout_secs == 0
            || self.ingest.process_timeout_secs > raggy_docs::limits::MAX_PROCESS_TIMEOUT_SECS
        {
            return Err(CoreError::InvalidConfig(format!(
                "el tiempo máximo de herramienta debe estar entre 1 y {} segundos",
                raggy_docs::limits::MAX_PROCESS_TIMEOUT_SECS
            )));
        }
        if !(raggy_docs::limits::MIN_MAX_OCR_IMAGE_PIXELS
            ..=raggy_docs::limits::MAX_MAX_OCR_IMAGE_PIXELS)
            .contains(&self.ingest.max_ocr_image_pixels)
        {
            return Err(CoreError::InvalidConfig(format!(
                "el máximo de píxeles OCR debe estar entre {} y {}",
                raggy_docs::limits::MIN_MAX_OCR_IMAGE_PIXELS,
                raggy_docs::limits::MAX_MAX_OCR_IMAGE_PIXELS
            )));
        }
        if self.ingest.min_text_chars > raggy_docs::limits::MAX_MIN_TEXT_CHARS {
            return Err(CoreError::InvalidConfig(format!(
                "el mínimo de texto por página no puede superar {} caracteres",
                raggy_docs::limits::MAX_MIN_TEXT_CHARS
            )));
        }
        Ok(())
    }
}

/// Escribe settings mediante un archivo temporal privado en el mismo directorio
/// y lo sustituye atómicamente para no dejar JSON parcial ante un cierre abrupto.
pub(crate) fn write_settings_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    restrict_permissions(path);
    Ok(())
}

/// Zeroiza los valores del campo `api_keys` de un documento JSON mutable.
fn zeroize_api_keys(document: &mut serde_json::Value) {
    if let Some(keys) = document
        .get_mut("api_keys")
        .and_then(serde_json::Value::as_object_mut)
    {
        for value in keys.values_mut() {
            if let serde_json::Value::String(secret) = value {
                secret.zeroize();
            }
        }
    }
}

/// Restringe los permisos del archivo de configuración (solo el propietario).
#[cfg(unix)]
pub(crate) fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    if let Ok(metadata) = std::fs::metadata(path) {
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o600);
        let _ = std::fs::set_permissions(path, permissions);
    }
}

#[cfg(not(unix))]
pub(crate) fn restrict_permissions(_path: &Path) {}

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
    fn avisa_sobre_valores_extremos() {
        assert!(AppConfig::default().rag.warnings().is_empty());

        let config = AppConfig {
            rag: RagConfig {
                chunk_size: 50,
                top_k: 40,
                ..RagConfig::default()
            },
            ..AppConfig::default()
        };
        assert_eq!(config.rag.warnings().len(), 2);
    }

    #[test]
    fn el_aviso_de_privacidad_empieza_pendiente() {
        assert_eq!(AppConfig::default().privacy_notice_version, 0);
    }

    #[test]
    fn configuracion_antigua_vuelve_a_mostrar_el_aviso_actualizado() {
        let old: AppConfig =
            serde_json::from_str(r#"{"data_notice_acknowledged":true,"theme":"dark"}"#)
                .expect("configuración anterior compatible");
        assert_eq!(old.theme, Theme::Dark);
        assert_eq!(old.privacy_notice_version, 0);
        assert_eq!(old.ingest.max_pdf_page_text_mb, 8);
        assert_eq!(old.ingest.max_pdf_text_mb, 64);
    }

    #[test]
    fn valida_limites_de_ingesta() {
        let config = AppConfig {
            ingest: IngestConfig {
                max_pages: 0,
                ..IngestConfig::default()
            },
            ..AppConfig::default()
        };
        assert!(config.validate().is_err());

        let config = AppConfig {
            ingest: IngestConfig {
                max_pdf_page_text_mb: 65,
                max_pdf_text_mb: 64,
                ..IngestConfig::default()
            },
            ..AppConfig::default()
        };
        assert!(config.validate().is_err());

        let config = AppConfig {
            ingest: IngestConfig {
                process_timeout_secs: 0,
                ..IngestConfig::default()
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
        let path = dir.join("settings.json");

        let mut config = AppConfig {
            theme: Theme::Dark,
            ..AppConfig::default()
        };
        config
            .api_keys
            .insert("deepseek".to_string(), "clave-de-prueba".to_string());
        config.save(&path).expect("guarda");

        let raw = std::fs::read_to_string(&path).expect("lee");
        assert!(raw.contains("\"deepseek\""), "debe ser JSON legible");

        let loaded = AppConfig::load(&path).expect("carga");
        assert_eq!(loaded.theme, Theme::Dark);
        assert_eq!(
            loaded.api_keys.get("deepseek").map(String::as_str),
            Some("clave-de-prueba")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn guardar_preferencias_conserva_las_claves_mas_recientes_del_almacen() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("raggy-config-keys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("crea carpeta");
        let path = dir.join("settings.json");

        std::fs::write(
            &path,
            r#"{"theme":"dark","api_keys":{"deepseek":"clave-actual"}}"#,
        )
        .expect("prepara configuración con clave reciente");

        let mut stale = AppConfig::default();
        stale
            .api_keys
            .insert("deepseek".to_string(), "clave-antigua".to_string());
        stale.theme = Theme::Light;
        stale.save(&path).expect("guarda preferencias");

        let loaded = AppConfig::load(&path).expect("recarga");
        assert_eq!(loaded.theme, Theme::Light);
        assert_eq!(
            loaded.api_keys.get("deepseek").map(String::as_str),
            Some("clave-actual"),
            "guardar preferencias no debe restaurar una copia antigua de la clave"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn guardar_preferencias_no_resucita_una_clave_eliminada() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("raggy-config-deleted-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("crea carpeta");
        let path = dir.join("settings.json");

        std::fs::write(&path, r#"{"api_keys":{}}"#).expect("clave ya eliminada");
        let mut stale = AppConfig::default();
        stale
            .api_keys
            .insert("openai".to_string(), "clave-eliminada".to_string());
        stale.save(&path).expect("guarda preferencias");

        let loaded = AppConfig::load(&path).expect("recarga");
        assert!(!loaded.api_keys.contains_key("openai"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn restringe_los_permisos_del_archivo() {
        use std::os::unix::fs::PermissionsExt;

        let mut dir = std::env::temp_dir();
        dir.push(format!("raggy-config-perms-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("settings.json");

        AppConfig::default().save(&path).expect("guarda");
        let mode = std::fs::metadata(&path)
            .expect("metadatos")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "el archivo con credenciales debe ser privado");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
