//! Tipos normalizados de mensajes, comunes a todos los proveedores.

use serde::{Deserialize, Serialize};

/// Rol de un mensaje dentro de una conversación.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Instrucciones de sistema (prompt estricto, HU-14).
    System,
    /// Mensaje del usuario.
    User,
    /// Respuesta del modelo.
    Assistant,
}

/// Mensaje de una conversación.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Rol del emisor.
    pub role: Role,
    /// Contenido textual.
    pub content: String,
}

impl ChatMessage {
    /// Construye un mensaje de sistema.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }

    /// Construye un mensaje de usuario.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    /// Construye un mensaje del asistente.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// Petición de chat, independiente del proveedor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    /// Identificador del modelo (p. ej. `deepseek-chat`).
    pub model: String,
    /// Historial de mensajes, incluido el de sistema.
    pub messages: Vec<ChatMessage>,
    /// Temperatura de muestreo (0.1 por defecto, HU-13).
    pub temperature: f32,
    /// Límite de tokens de salida.
    pub max_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_constructores_fijan_el_rol() {
        assert_eq!(ChatMessage::system("x").role, Role::System);
        assert_eq!(ChatMessage::user("x").role, Role::User);
        assert_eq!(ChatMessage::assistant("x").role, Role::Assistant);
    }
}
