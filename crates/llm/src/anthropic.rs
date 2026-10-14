//! Adaptador para la API *Messages* de Anthropic (streaming SSE).

use std::io::{BufRead, BufReader};

use reqwest::blocking::Client;
use serde_json::json;

use crate::openai::{build_client, describe_status, model_ids, shorten, unavailable};
use crate::provider::{ChatDelta, LlmProvider, ProviderError, ProviderId};
use crate::sse;
use crate::types::{ChatRequest, Role};

const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Cliente para la API de Anthropic.
pub struct AnthropicProvider {
    api_key: String,
    client: Client,
}

impl Drop for AnthropicProvider {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.api_key.zeroize();
    }
}

impl AnthropicProvider {
    /// Crea el adaptador con la API Key indicada.
    pub fn new(api_key: String) -> Result<Self, ProviderError> {
        Ok(Self {
            api_key,
            client: build_client()?,
        })
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{ANTHROPIC_URL}/{path}")
    }
}

impl LlmProvider for AnthropicProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Anthropic
    }

    fn check(&self) -> Result<(), ProviderError> {
        if self.api_key.trim().is_empty() {
            return Err(ProviderError::MissingApiKey {
                provider: ProviderId::Anthropic.display_name(),
                env_var: ProviderId::Anthropic.api_key_env(),
            });
        }
        Ok(())
    }

    fn check_connection(&self) -> Result<Vec<String>, ProviderError> {
        let id = ProviderId::Anthropic;
        let response = self
            .client
            .get(self.endpoint("models"))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .send()
            .map_err(|error| unavailable(id, &error))?;

        let status = response.status();
        let body = response.text().unwrap_or_default();
        if !status.is_success() {
            return Err(describe_status(id, status, &body));
        }

        let value: serde_json::Value = serde_json::from_str(&body)
            .map_err(|error| ProviderError::Protocol(error.to_string()))?;
        Ok(model_ids(&value))
    }

    fn stream_chat(
        &self,
        request: &ChatRequest,
        on_delta: &mut dyn FnMut(ChatDelta) -> bool,
    ) -> Result<(), ProviderError> {
        let id = ProviderId::Anthropic;

        let system: String = request
            .messages
            .iter()
            .filter(|message| message.role == Role::System)
            .map(|message| message.content.clone())
            .collect::<Vec<_>>()
            .join("\n\n");

        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter(|message| message.role != Role::System)
            .map(|message| json!({ "role": message.role.key(), "content": message.content }))
            .collect();

        let mut body = json!({
            "model": request.model,
            "max_tokens": request.max_tokens,
            "temperature": request.temperature,
            "messages": messages,
            "stream": true,
        });
        if !system.is_empty() {
            body["system"] = json!(system);
        }

        let response = self
            .client
            .post(self.endpoint("messages"))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .map_err(|error| unavailable(id, &error))?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().unwrap_or_default();
            return Err(describe_status(id, status, &text));
        }

        for line in BufReader::new(response).lines() {
            let line = line.map_err(|error| ProviderError::Unavailable {
                provider: id.display_name(),
                message: format!("la conexión se interrumpió: {error}"),
            })?;
            let Some(data) = sse::data_of(&line) else {
                continue;
            };

            let value: serde_json::Value = serde_json::from_str(data)
                .map_err(|error| ProviderError::Protocol(error.to_string()))?;

            match parse_event(&value) {
                StreamEvent::Text(text) => {
                    if !text.is_empty() && !on_delta(ChatDelta::Content(text.to_string())) {
                        return Err(ProviderError::Cancelled);
                    }
                }
                StreamEvent::Stop => break,
                StreamEvent::Error(message) => {
                    return Err(ProviderError::Http {
                        provider: id.display_name(),
                        message: shorten(&message),
                    });
                }
                StreamEvent::Ignore => {}
            }
        }

        let _ = on_delta(ChatDelta::Done);
        Ok(())
    }
}

/// Evento relevante extraído del stream de Anthropic.
#[derive(Debug, PartialEq, Eq)]
enum StreamEvent<'a> {
    Text(&'a str),
    Stop,
    Error(String),
    Ignore,
}

/// Interpreta un evento del stream de Anthropic.
fn parse_event(value: &serde_json::Value) -> StreamEvent<'_> {
    match value["type"].as_str() {
        Some("content_block_delta") => {
            StreamEvent::Text(value["delta"]["text"].as_str().unwrap_or(""))
        }
        Some("message_stop") => StreamEvent::Stop,
        Some("error") => StreamEvent::Error(value["error"].to_string()),
        _ => StreamEvent::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpreta_deltas_de_texto() {
        let value: serde_json::Value = serde_json::from_str(
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hola"}}"#,
        )
        .expect("json");
        assert_eq!(parse_event(&value), StreamEvent::Text("Hola"));
    }

    #[test]
    fn reconoce_el_final_y_los_errores() {
        let stop: serde_json::Value =
            serde_json::from_str(r#"{"type":"message_stop"}"#).expect("json");
        assert_eq!(parse_event(&stop), StreamEvent::Stop);

        let error: serde_json::Value = serde_json::from_str(
            r#"{"type":"error","error":{"type":"overloaded_error","message":"ocupado"}}"#,
        )
        .expect("json");
        assert!(matches!(parse_event(&error), StreamEvent::Error(_)));

        let other: serde_json::Value =
            serde_json::from_str(r#"{"type":"message_start"}"#).expect("json");
        assert_eq!(parse_event(&other), StreamEvent::Ignore);
    }
}
