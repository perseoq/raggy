//! Adaptador para proveedores compatibles con la API de *Chat Completions* de
//! OpenAI (OpenAI, DeepSeek y OpenRouter).

use std::io::{BufRead, BufReader};
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::StatusCode;
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

impl Drop for OpenAiCompatibleProvider {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.api_key.zeroize();
    }
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

    fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path)
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
        .map_err(|error| ProviderError::Protocol(error.to_string()))
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

    fn check_connection(&self) -> Result<Vec<String>, ProviderError> {
        let response = self
            .client
            .get(self.endpoint("models"))
            .bearer_auth(&self.api_key)
            .send()
            .map_err(|error| unavailable(self.id, &error))?;

        let status = response.status();
        let body = response.text().unwrap_or_default();
        if !status.is_success() {
            return Err(describe_status(self.id, status, &body));
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
            .post(self.endpoint("chat/completions"))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .map_err(|error| unavailable(self.id, &error))?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().unwrap_or_default();
            return Err(describe_status(self.id, status, &text));
        }

        for line in BufReader::new(response).lines() {
            let line = line.map_err(|error| ProviderError::Unavailable {
                provider: self.id.display_name(),
                message: format!("la conexión se interrumpió: {error}"),
            })?;
            let Some(data) = sse::data_of(&line) else {
                continue;
            };
            if data == "[DONE]" {
                break;
            }

            let value: serde_json::Value = serde_json::from_str(data)
                .map_err(|error| ProviderError::Protocol(error.to_string()))?;

            if let Some(error) = value.get("error") {
                return Err(describe_stream_error(self.id, error));
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

/// Extrae los identificadores de modelo de la respuesta de `/models`.
pub(crate) fn model_ids(value: &serde_json::Value) -> Vec<String> {
    value
        .get("data")
        .and_then(|data| data.as_array())
        .map(|models| {
            models
                .iter()
                .filter_map(|model| model.get("id").and_then(|id| id.as_str()))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Traduce un error de red a un fallo de disponibilidad.
pub(crate) fn unavailable(id: ProviderId, error: &reqwest::Error) -> ProviderError {
    ProviderError::Unavailable {
        provider: id.display_name(),
        message: if error.is_timeout() {
            "la conexión ha caducado".to_string()
        } else if error.is_connect() {
            "no se pudo conectar (comprueba tu red)".to_string()
        } else {
            shorten(&error.to_string())
        },
    }
}

/// Traduce un código de estado HTTP a un mensaje claro (HU-24).
pub(crate) fn describe_status(id: ProviderId, status: StatusCode, body: &str) -> ProviderError {
    let provider = id.display_name();
    match status.as_u16() {
        401 | 403 => ProviderError::Http {
            provider,
            message: format!("API Key inválida o sin permisos (HTTP {status})"),
        },
        402 => ProviderError::Http {
            provider,
            message: "la cuenta no tiene crédito suficiente (HTTP 402)".to_string(),
        },
        404 => ProviderError::Http {
            provider,
            message: format!("recurso no encontrado, revisa el modelo o la URL (HTTP {status})"),
        },
        429 => ProviderError::Unavailable {
            provider,
            message: "se ha alcanzado el límite de peticiones (HTTP 429)".to_string(),
        },
        code if (500..=599).contains(&code) => ProviderError::Unavailable {
            provider,
            message: format!("el proveedor no está disponible (HTTP {status})"),
        },
        _ => ProviderError::Http {
            provider,
            message: format!("HTTP {status}: {}", shorten(body)),
        },
    }
}

/// Traduce un error embebido en el stream a un error tipado.
fn describe_stream_error(id: ProviderId, error: &serde_json::Value) -> ProviderError {
    let message = error
        .get("message")
        .and_then(|message| message.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| shorten(&error.to_string()));

    ProviderError::Http {
        provider: id.display_name(),
        message,
    }
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
    fn extrae_identificadores_de_modelo() {
        let value: serde_json::Value =
            serde_json::from_str(r#"{"data":[{"id":"a"},{"id":"b"}]}"#).expect("json");
        assert_eq!(model_ids(&value), vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn traduce_los_errores_http() {
        let id = ProviderId::OpenAi;

        assert!(matches!(
            describe_status(id, StatusCode::UNAUTHORIZED, ""),
            ProviderError::Http { .. }
        ));
        assert!(!describe_status(id, StatusCode::UNAUTHORIZED, "").is_availability());

        let limit = describe_status(id, StatusCode::TOO_MANY_REQUESTS, "");
        assert!(limit.is_availability());

        let server = describe_status(id, StatusCode::BAD_GATEWAY, "");
        assert!(server.is_availability());
    }

    #[test]
    fn recorta_mensajes_largos() {
        let long = "x".repeat(MAX_ERROR_BODY + 50);
        let shortened = shorten(&long);
        assert!(shortened.ends_with('…'));
        assert_eq!(shortened.chars().count(), MAX_ERROR_BODY + 1);
    }
}
