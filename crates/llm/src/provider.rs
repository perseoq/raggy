//! Contrato común de proveedores de LLM.

use serde::{Deserialize, Serialize};

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

    /// Interpreta una clave almacenada en configuración.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == key)
    }
}

/// Errores comunes a los adaptadores de proveedor.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// Falta la API Key del proveedor (HU-23).
    #[error("el proveedor `{provider}` no tiene API Key configurada")]
    MissingApiKey {
        /// Clave del proveedor.
        provider: &'static str,
    },

    /// La funcionalidad aún no está implementada en esta fase.
    #[error("función no disponible todavía en esta fase del proyecto")]
    NotImplemented,

    /// Cualquier otro error encapsulado.
    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

/// Contrato que deberá cumplir cada adaptador de proveedor.
///
/// Se mantiene deliberadamente pequeño en la Fase 0; el streaming y la llamada
/// de completado se añadirán en la Fase 3.
pub trait LlmProvider: Send + Sync {
    /// Identificador del proveedor.
    fn id(&self) -> ProviderId;

    /// Nombre legible; por defecto, el del identificador.
    fn display_name(&self) -> &'static str {
        self.id().display_name()
    }

    /// Verifica que la configuración permite usar el proveedor (HU-24).
    fn check(&self) -> Result<(), ProviderError>;
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
}
