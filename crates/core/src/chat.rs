//! Chat con streaming sobre una sesión (HU-06, HU-12, HU-13, HU-14).
//!
//! Igual que el indexador, el chat se ejecuta en un hilo dedicado para no
//! bloquear la interfaz. El hilo recibe peticiones ya compuestas (incluido el
//! prompt de sistema con el contexto recuperado de la carpeta de la sesión),
//! emite los fragmentos en streaming y persiste la respuesta final con sus
//! citas.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use raggy_llm::{
    build_system_prompt, ArtifactContext, ChatDelta, ChatMessage, ChatRequest, ContextBlock,
    LlmProvider, ProviderError, ProviderId,
};
use raggy_rag::{Citation, RetrievedChunk};
use raggy_storage::{Message, Storage};

use crate::error::{CoreError, Result};

/// Crea el proveedor a partir de su identificador.
///
/// Se inyecta desde fuera para poder usar el proveedor simulado en pruebas.
pub type ProviderFactory = Arc<
    dyn Fn(ProviderId) -> std::result::Result<Box<dyn LlmProvider>, ProviderError> + Send + Sync,
>;

/// Convierte los fragmentos recuperados en bloques de contexto y citas.
pub fn context_from_retrieved(chunks: &[RetrievedChunk]) -> (Vec<ContextBlock>, Vec<Citation>) {
    let blocks = chunks
        .iter()
        .map(|chunk| ContextBlock::new(chunk.citation().label(), chunk.chunk.content.clone()))
        .collect();
    let citations = chunks.iter().map(RetrievedChunk::citation).collect();
    (blocks, citations)
}

/// Compone la petición de chat: prompt de sistema estricto (HU-14), memoria de
/// artefactos (HU-21), historial de la sesión y mensaje del usuario.
pub fn compose_request(
    model: &str,
    temperature: f32,
    max_tokens: u32,
    context: &[ContextBlock],
    artifacts: &[ArtifactContext],
    history: &[Message],
    user_message: &str,
) -> ChatRequest {
    let mut messages = vec![ChatMessage::system(build_system_prompt(context, artifacts))];
    for message in history {
        let role = match message.role.as_str() {
            "user" => raggy_llm::Role::User,
            "assistant" => raggy_llm::Role::Assistant,
            _ => continue,
        };
        messages.push(ChatMessage {
            role,
            content: message.content.clone(),
        });
    }
    messages.push(ChatMessage::user(user_message));

    ChatRequest {
        model: model.to_string(),
        messages,
        temperature,
        max_tokens,
    }
}

/// Comandos aceptados por el hilo de chat.
pub enum ChatCommand {
    /// Genera una respuesta para una sesión.
    Generate {
        /// Sesión destino.
        session_id: String,
        /// Proveedor a usar.
        provider: ProviderId,
        /// Petición ya compuesta.
        request: ChatRequest,
        /// Fuentes recuperadas que se citarán en la respuesta.
        citations: Vec<Citation>,
    },
    /// Detiene la generación en curso.
    Cancel,
    /// Finaliza el hilo.
    Shutdown,
}

/// Eventos emitidos por el hilo de chat.
#[derive(Debug, Clone)]
pub enum ChatEvent {
    /// Fragmento de texto generado.
    Delta {
        /// Sesión en curso.
        session_id: String,
        /// Texto parcial.
        text: String,
    },
    /// Generación terminada y mensaje persistido.
    Finished {
        /// Sesión afectada.
        session_id: String,
        /// Mensaje del asistente guardado.
        message: Message,
    },
    /// Generación fallida.
    Failed {
        /// Sesión afectada.
        session_id: String,
        /// Descripción del error.
        message: String,
        /// Si el fallo es de disponibilidad del proveedor (HU-25).
        availability: bool,
    },
}

/// Hilo dedicado que ejecuta las generaciones fuera del hilo de la interfaz.
pub struct ChatWorker {
    command_tx: Sender<ChatCommand>,
    event_rx: Receiver<ChatEvent>,
    cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ChatWorker {
    /// Arranca el hilo de chat con su propia conexión a la base de datos.
    pub fn spawn(db_path: PathBuf, factory: ProviderFactory) -> Result<Self> {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);

        let handle = thread::Builder::new()
            .name("raggy-chat".to_string())
            .spawn(move || worker_loop(&db_path, factory, worker_cancel, command_rx, event_tx))
            .map_err(|source| CoreError::Io {
                path: PathBuf::from("<hilo de chat>"),
                source,
            })?;

        Ok(Self {
            command_tx,
            event_rx,
            cancel,
            handle: Some(handle),
        })
    }

    /// Encola una generación para la sesión indicada.
    pub fn generate(
        &self,
        session_id: impl Into<String>,
        provider: ProviderId,
        request: ChatRequest,
        citations: Vec<Citation>,
    ) {
        self.cancel.store(false, Ordering::SeqCst);
        let _ = self.command_tx.send(ChatCommand::Generate {
            session_id: session_id.into(),
            provider,
            request,
            citations,
        });
    }

    /// Solicita detener la generación en curso (HU-12).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Extrae todos los eventos disponibles sin bloquear.
    pub fn drain(&self) -> Vec<ChatEvent> {
        self.event_rx.try_iter().collect()
    }
}

impl Drop for ChatWorker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
        let _ = self.command_tx.send(ChatCommand::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn worker_loop(
    db_path: &Path,
    factory: ProviderFactory,
    cancel: Arc<AtomicBool>,
    command_rx: Receiver<ChatCommand>,
    event_tx: Sender<ChatEvent>,
) {
    let mut storage = match Storage::open(db_path) {
        Ok(storage) => storage,
        Err(error) => {
            tracing::error!("el hilo de chat no pudo abrir la base de datos: {error}");
            return;
        }
    };

    while let Ok(command) = command_rx.recv() {
        match command {
            ChatCommand::Shutdown => break,
            ChatCommand::Cancel => cancel.store(true, Ordering::SeqCst),
            ChatCommand::Generate {
                session_id,
                provider,
                request,
                citations,
            } => {
                cancel.store(false, Ordering::SeqCst);
                generate(
                    &mut storage,
                    &factory,
                    &cancel,
                    &event_tx,
                    session_id,
                    provider,
                    request,
                    citations,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn generate(
    storage: &mut Storage,
    factory: &ProviderFactory,
    cancel: &AtomicBool,
    event_tx: &Sender<ChatEvent>,
    session_id: String,
    provider: ProviderId,
    request: ChatRequest,
    citations: Vec<Citation>,
) {
    let provider_impl = match factory(provider) {
        Ok(provider) => provider,
        Err(error) => {
            let _ = event_tx.send(ChatEvent::Failed {
                session_id,
                availability: error.is_availability(),
                message: error.to_string(),
            });
            return;
        }
    };

    let mut text = String::new();
    let stream_session = session_id.clone();
    let result = provider_impl.stream_chat(&request, &mut |delta| match delta {
        ChatDelta::Content(piece) => {
            text.push_str(&piece);
            let _ = event_tx.send(ChatEvent::Delta {
                session_id: stream_session.clone(),
                text: piece,
            });
            !cancel.load(Ordering::SeqCst)
        }
        ChatDelta::Done => true,
    });

    match result {
        Ok(()) | Err(ProviderError::Cancelled) => {
            if text.trim().is_empty() {
                let _ = event_tx.send(ChatEvent::Failed {
                    session_id,
                    availability: false,
                    message: "La generación no produjo contenido.".to_string(),
                });
                return;
            }

            let anchored = !citations.is_empty();
            let message = Message::new(&session_id, "assistant", &text, anchored, citations);
            if let Err(error) = storage.add_message(&message) {
                let _ = event_tx.send(ChatEvent::Failed {
                    session_id,
                    availability: false,
                    message: format!("no se pudo guardar la respuesta: {error}"),
                });
                return;
            }
            let _ = storage.touch_session(&session_id);
            let _ = event_tx.send(ChatEvent::Finished {
                session_id,
                message,
            });
        }
        Err(error) => {
            let _ = event_tx.send(ChatEvent::Failed {
                session_id,
                availability: error.is_availability(),
                message: error.to_string(),
            });
        }
    }
}
