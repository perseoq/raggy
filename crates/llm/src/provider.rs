//! Contrato común de proveedores de LLM y su factoría.

use serde::{Deserialize, Serialize};

use crate::anthropic::AnthropicProvider;
use crate::openai::OpenAiCompatibleProvider;
use crate::types::ChatRequest;

/// Identificador de proveedor soportado (HU-15, HU-23).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    /// OpenAI.
    OpenAi,
    /// Anthropic.
    Anthropic,
    /// DeepSeek.
    DeepSeek,
    /// OpenRouter (agregador).
    OpenRouter,
}

impl ProviderId {
    /// Todos los proveedores soportados, en orden de presentación.
    pub const ALL: [ProviderId; 4] = [
        ProviderId::OpenAi,
        ProviderId::Anthropic,
        ProviderId::DeepSeek,
        ProviderId::OpenRouter,
    ];

    /// Clave estable usada en configuración y base de datos.
    pub fn key(self) -> &'static str {
        match self {
            ProviderId::OpenAi => "openai",
            ProviderId::Anthropic => "anthropic",
            ProviderId::DeepSeek => "deepseek",
            ProviderId::OpenRouter => "openrouter",
        }
    }

    /// Nombre legible para la interfaz.
    pub fn display_name(self) -> &'static str {
        match self {
            ProviderId::OpenAi => "OpenAI",
            ProviderId::Anthropic => "Anthropic",
            ProviderId::DeepSeek => "DeepSeek",
            ProviderId::OpenRouter => "OpenRouter",
        }
    }

    /// Variable de entorno usada como alternativa al llavero del sistema.
    pub fn api_key_env(self) -> &'static str {
        match self {
            ProviderId::OpenAi => "OPENAI_API_KEY",
            ProviderId::Anthropic => "ANTHROPIC_API_KEY",
            ProviderId::DeepSeek => "DEEPSEEK_API_KEY",
            ProviderId::OpenRouter => "OPENROUTER_API_KEY",
        }
    }

    /// Modelo por defecto para sesiones nuevas.
    pub fn default_model(self) -> &'static str {
        match self {
            ProviderId::OpenAi => "gpt-4o-mini",
            ProviderId::Anthropic => "claude-3-5-sonnet-latest",
            ProviderId::DeepSeek => "deepseek-chat",
            ProviderId::OpenRouter => "openai/gpt-4o-mini",
        }
    }

    /// Modelos sugeridos para el selector.
    pub fn models(self) -> &'static [&'static str] {
        match self {
            ProviderId::OpenAi => &["gpt-4o-mini", "gpt-4o", "gpt-4.1-mini"],
            ProviderId::Anthropic => &[
                "claude-3-5-sonnet-latest",
                "claude-3-5-haiku-latest",
                "claude-3-opus-latest",
            ],
            ProviderId::DeepSeek => &["deepseek-chat", "deepseek-flash", "deepseek-v4-pro"],
            ProviderId::OpenRouter => &[
                "openai/gpt-4o-mini",
                "anthropic/claude-3.5-sonnet",
                "google/gemini-flash-1.5",
            ],
        }
    }

    /// Interpreta una clave almacenada en configuración.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|provider| provider.key() == key)
    }
}

/// Evento emitido durante la generación.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatDelta {
    /// Fragmento de texto generado.
    Content(String),
    /// Fin de la generación.
    Done,
}

/// Errores comunes a los adaptadores de proveedor.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// Falta la API Key del proveedor (HU-15).
    #[error("el proveedor `{provider}` no tiene API Key configurada (define {env_var})")]
    MissingApiKey {
        /// Nombre del proveedor.
        provider: &'static str,
        /// Variable de entorno esperada.
        env_var: &'static str,
    },

    /// La funcionalidad aún no está implementada en esta fase.
    #[error("el proveedor `{provider}` no está disponible todavía")]
    NotImplemented {
        /// Nombre del proveedor.
        provider: &'static str,
    },

    /// El proveedor no responde (red, rate limit o error del servidor).
    ///
    /// Importa para HU-25: solo estos fallos ofrecen el proveedor de respaldo.
    #[error("el proveedor `{provider}` no está disponible: {message}")]
    Unavailable {
        /// Nombre del proveedor.
        provider: &'static str,
        /// Detalle del problema.
        message: String,
    },

    /// Error de la API del proveedor (clave inválida, petición inválida, etc.).
    #[error("error del proveedor `{provider}`: {message}")]
    Http {
        /// Nombre del proveedor.
        provider: &'static str,
        /// Detalle del problema.
        message: String,
    },

    /// Respuesta inesperada del proveedor.
    #[error("respuesta inesperada del proveedor: {0}")]
    Protocol(String),

    /// El usuario detuvo la generación (HU-12).
    #[error("generación detenida por el usuario")]
    Cancelled,
}

impl ProviderError {
    /// ¿Es un fallo de disponibilidad del proveedor? (HU-25)
    pub fn is_availability(&self) -> bool {
        matches!(self, ProviderError::Unavailable { .. })
    }
}

/// Contrato que cumple cada adaptador de proveedor.
pub trait LlmProvider: Send + Sync {
    /// Identificador del proveedor.
    fn id(&self) -> ProviderId;

    /// Nombre legible; por defecto, el del identificador.
    fn display_name(&self) -> &'static str {
        self.id().display_name()
    }

    /// Verifica que la configuración permite usar el proveedor (HU-24).
    fn check(&self) -> Result<(), ProviderError>;

    /// Comprueba la conexión y devuelve los modelos disponibles (HU-24).
    fn check_connection(&self) -> Result<Vec<String>, ProviderError>;

    /// Genera una respuesta en streaming (HU-12).
    ///
    /// Invoca `on_delta` con cada fragmento; devolver `false` desde el cierre
    /// cancela la generación.
    fn stream_chat(
        &self,
        request: &ChatRequest,
        on_delta: &mut dyn FnMut(ChatDelta) -> bool,
    ) -> Result<(), ProviderError>;
}

/// API Key del proveedor leída del entorno, si existe.
pub fn api_key(id: ProviderId) -> Option<String> {
    std::env::var(id.api_key_env())
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// ¿Hay credenciales configuradas en el entorno para el proveedor?
pub fn is_configured(id: ProviderId) -> bool {
    api_key(id).is_some()
}

/// Crea el adaptador del proveedor a partir de su API Key.
pub fn from_api_key(
    id: ProviderId,
    api_key: String,
) -> Result<Box<dyn LlmProvider>, ProviderError> {
    if api_key.trim().is_empty() {
        return Err(ProviderError::MissingApiKey {
            provider: id.display_name(),
            env_var: id.api_key_env(),
        });
    }

    let provider: Box<dyn LlmProvider> = match id {
        ProviderId::Anthropic => Box::new(AnthropicProvider::new(api_key)?),
        ProviderId::OpenAi | ProviderId::DeepSeek | ProviderId::OpenRouter => {
            Box::new(OpenAiCompatibleProvider::new(id, api_key)?)
        }
    };
    Ok(provider)
}

/// Crea el adaptador del proveedor leyendo su API Key del entorno.
pub fn from_env(id: ProviderId) -> Result<Box<dyn LlmProvider>, ProviderError> {
    let api_key = api_key(id).ok_or(ProviderError::MissingApiKey {
        provider: id.display_name(),
        env_var: id.api_key_env(),
    })?;
    from_api_key(id, api_key)
}

/// Realiza una llamada mínima para validar una API Key (HU-24).
pub fn check_connection_with_key(
    id: ProviderId,
    api_key: &str,
) -> Result<Vec<String>, ProviderError> {
    from_api_key(id, api_key.to_string())?.check_connection()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_clave_y_el_identificador_son_biyectivos() {
        for provider in ProviderId::ALL {
            assert_eq!(ProviderId::from_key(provider.key()), Some(provider));
        }
        assert_eq!(ProviderId::from_key("inexistente"), None);
    }

    #[test]
    fn sin_api_key_informa_del_error() {
        match from_api_key(ProviderId::OpenAi, String::new()) {
            Err(error) => {
                assert!(matches!(error, ProviderError::MissingApiKey { .. }));
                assert!(!error.is_availability());
            }
            Ok(_) => panic!("debería faltar la clave"),
        }
    }

    #[test]
    fn clasifica_los_fallos_de_disponibilidad() {
        let unavailable = ProviderError::Unavailable {
            provider: "OpenAI",
            message: "HTTP 503".to_string(),
        };
        assert!(unavailable.is_availability());

        let invalid = ProviderError::Http {
            provider: "OpenAI",
            message: "HTTP 401".to_string(),
        };
        assert!(!invalid.is_availability());
    }
}
