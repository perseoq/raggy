//! `localmind-llm` — capa de abstracción multi-proveedor.
//!
//! En la Fase 0 este crate define únicamente el contrato común ([`LlmProvider`])
//! y los tipos normalizados de mensajes. Los adaptadores concretos (OpenAI,
//! Anthropic, DeepSeek, OpenRouter) se implementarán en la Fase 3-4 (HU-15,
//! HU-23, HU-24, HU-25).

pub mod provider;
pub mod types;

pub use provider::{LlmProvider, ProviderError, ProviderId};
pub use types::{ChatMessage, ChatRequest, Role};
