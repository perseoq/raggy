//! `raggy-llm` — capa de abstracción multi-proveedor.
//!
//! Normaliza los mensajes, el streaming y los errores de OpenAI, Anthropic,
//! DeepSeek y OpenRouter tras un contrato común ([`LlmProvider`]). Incluye la
//! construcción del prompt de sistema estricto (HU-14) y un proveedor simulado
//! sin red para pruebas y demostración.

pub mod anthropic;
pub mod code;
pub mod mock;
pub mod openai;
pub mod prompt;
pub mod provider;
pub mod sse;
pub mod types;

pub use anthropic::AnthropicProvider;
pub use code::{extension_for, extract_code_blocks, suggest_filename, CodeBlock};
pub use mock::MockProvider;
pub use openai::OpenAiCompatibleProvider;
pub use prompt::{build_system_prompt, ArtifactContext, ContextBlock, STRICT_SYSTEM_PROMPT};
pub use provider::{
    api_key, check_connection_with_key, from_api_key, from_env, is_configured, ChatDelta,
    LlmProvider, ProviderError, ProviderId,
};
pub use types::{ChatMessage, ChatRequest, Role};
