//! Adaptador para proveedores compatibles con la API de *Chat Completions* de
//! OpenAI (OpenAI, DeepSeek y OpenRouter).

use std::io::{BufRead, BufReader};
use std::time::Duration;

use reqwest::blocking::Client;
use serde_json::json;

use crate::provider::{ChatDelta, LlmProvider, ProviderError, ProviderId};
use crate::sse;
use crate::types::ChatRequest;

/// Longitud máxima del cuerpo de error que se muestra al usuario.
const MAX_ERROR_BODY: usize = 400;

/// Cliente para cualquier API compatible con OpenAI.
pub struct OpenAiCompatibleProvider {
    id: ProviderId,
    api_key: String,
    base_url: &'static str,
    client: Client,
}

impl OpenAiCompatibleProvider {
    /// Crea el adaptador para el proveedor indicado.
    pub fn new(id: ProviderId, api_key: String) -> Result<Self, ProviderError> {
        Ok(Self {
            id,
            api_key,
            base_url: base_url(id),
            client: build_client()?,
        })
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }
}

fn base_url(id: ProviderId) -> &'static str {
    match id {
        ProviderId::OpenAi => "https://api.openai.com/v1",
        ProviderId::DeepSeek => "https://api.deepseek.com/v1",
        ProviderId::OpenRouter => "https://openrouter.ai/api/v1",
        ProviderId::Anthropic => "https://api.anthropic.com/v1",
    }
}

/// Cliente HTTP con tiempo de conexión acotado pero sin límite de lectura, para
/// no cortar las respuestas largas en streaming.
pub(crate) fn build_client() -> Result<Client, ProviderError> {
    Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| ProviderError::Http(error.to_string()))
}

impl LlmProvider for OpenAiCompatibleProvider {
    fn id(&self) -> ProviderId {
        self.id
    }

    fn check(&self) -> Result<(), ProviderError> {
        if self.api_key.trim().is_empty() {
            return Err(ProviderError::MissingApiKey {
                provider: self.id.display_name(),
                env_var: self.id.api_key_env(),
            });
        }
        Ok(())
    }

    fn stream_chat(
        &self,
        request: &ChatRequest,
        on_delta: &mut dyn FnMut(ChatDelta) -> bool,
    ) -> Result<(), ProviderError> {
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|message| json!({ "role": message.role.key(), "content": message.content }))
            .collect();

        let body = json!({
            "model": request.model,
            "messages": messages,
            "temperature": request.temperature,
            "max_tokens": request.max_tokens,
            "stream": true,
        });

        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .map_err(|error| ProviderError::Http(error.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().unwrap_or_default();
            return Err(ProviderError::Http(format!(
                "HTTP {status}: {}",
                shorten(&text)
            )));
        }

        for line in BufReader::new(response).lines() {
            let line = line.map_err(|error| ProviderError::Http(error.to_string()))?;
            let Some(data) = sse::data_of(&line) else {
                continue;
            };
            if data == "[DONE]" {
                break;
            }

            let value: serde_json::Value = serde_json::from_str(data)
                .map_err(|error| ProviderError::Protocol(error.to_string()))?;

            if let Some(error) = value.get("error") {
                return Err(ProviderError::Http(shorten(&error.to_string())));
            }

            if let Some(text) = delta_content(&value) {
                if !text.is_empty() && !on_delta(ChatDelta::Content(text.to_string())) {
                    return Err(ProviderError::Cancelled);
                }
            }
        }

        let _ = on_delta(ChatDelta::Done);
        Ok(())
    }
}

/// Extrae el texto incremental de un fragmento del stream de OpenAI.
fn delta_content(value: &serde_json::Value) -> Option<&str> {
    value
        .get("choices")?
        .as_array()?
        .first()?
        .get("delta")?
        .get("content")?
        .as_str()
}

/// Recorta un mensaje de error para mostrarlo de forma legible.
pub(crate) fn shorten(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= MAX_ERROR_BODY {
        text.to_string()
    } else {
        let mut shortened: String = text.chars().take(MAX_ERROR_BODY).collect();
        shortened.push('…');
        shortened
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrae_el_texto_de_un_fragmento() {
        let value: serde_json::Value = serde_json::from_str(
            r#"{"id":"x","choices":[{"index":0,"delta":{"content":"Hola"},"finish_reason":null}]}"#,
        )
        .expect("json válido");
        assert_eq!(delta_content(&value), Some("Hola"));
    }

    #[test]
    fn ignora_fragmentos_sin_texto() {
        let value: serde_json::Value =
            serde_json::from_str(r#"{"choices":[{"delta":{"role":"assistant"}}]}"#).expect("json");
        assert_eq!(delta_content(&value), None);

        let empty: serde_json::Value = serde_json::from_str(r#"{"choices":[]}"#).expect("json");
        assert_eq!(delta_content(&empty), None);
    }

    #[test]
    fn recorta_mensajes_largos() {
        let long = "x".repeat(MAX_ERROR_BODY + 50);
        let shortened = shorten(&long);
        assert!(shortened.ends_with('…'));
        assert_eq!(shortened.chars().count(), MAX_ERROR_BODY + 1);
    }
}
