//! Proveedor simulado, determinista y sin red.
//!
//! Se usa en las pruebas del pipeline de chat y para poder ejercitar la
//! interfaz sin credenciales ni conexión.

use crate::provider::{ChatDelta, LlmProvider, ProviderError, ProviderId};
use crate::types::{ChatRequest, Role};

/// Proveedor que devuelve una respuesta prefijada o generada a partir del
/// último mensaje del usuario.
pub struct MockProvider {
    id: ProviderId,
    reply: Option<String>,
}

impl MockProvider {
    /// Crea el proveedor simulado.
    pub fn new(id: ProviderId) -> Self {
        Self { id, reply: None }
    }

    /// Crea el proveedor simulado con una respuesta fija.
    pub fn with_reply(id: ProviderId, reply: impl Into<String>) -> Self {
        Self {
            id,
            reply: Some(reply.into()),
        }
    }

    fn answer(&self, request: &ChatRequest) -> String {
        if let Some(reply) = &self.reply {
            return reply.clone();
        }
        let user = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == Role::User)
            .map(|message| message.content.as_str())
            .unwrap_or("");
        format!("Respuesta simulada para: {user}")
    }
}

impl LlmProvider for MockProvider {
    fn id(&self) -> ProviderId {
        self.id
    }

    fn check(&self) -> Result<(), ProviderError> {
        Ok(())
    }

    fn check_connection(&self) -> Result<Vec<String>, ProviderError> {
        Ok(vec!["mock-model".to_string()])
    }

    fn stream_chat(
        &self,
        request: &ChatRequest,
        on_delta: &mut dyn FnMut(ChatDelta) -> bool,
    ) -> Result<(), ProviderError> {
        let answer = self.answer(request);
        for piece in answer.split_inclusive(' ') {
            if !on_delta(ChatDelta::Content(piece.to_string())) {
                return Err(ProviderError::Cancelled);
            }
        }
        let _ = on_delta(ChatDelta::Done);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ChatMessage;

    #[test]
    fn emite_trozos_y_termina() {
        let provider = MockProvider::with_reply(ProviderId::DeepSeek, "hola mundo");
        let request = ChatRequest {
            model: "mock".to_string(),
            messages: vec![ChatMessage::user("da igual")],
            temperature: 0.1,
            max_tokens: 16,
        };

        let mut text = String::new();
        let mut done = false;
        provider
            .stream_chat(&request, &mut |delta| {
                match delta {
                    ChatDelta::Content(piece) => text.push_str(&piece),
                    ChatDelta::Done => done = true,
                }
                true
            })
            .expect("genera");

        assert_eq!(text, "hola mundo");
        assert!(done);
    }

    #[test]
    fn se_puede_cancelar() {
        let provider = MockProvider::with_reply(ProviderId::DeepSeek, "una respuesta larga");
        let request = ChatRequest {
            model: "mock".to_string(),
            messages: vec![ChatMessage::user("x")],
            temperature: 0.1,
            max_tokens: 16,
        };

        let result = provider.stream_chat(&request, &mut |_| false);
        assert!(matches!(result, Err(ProviderError::Cancelled)));
    }
}
