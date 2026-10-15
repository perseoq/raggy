//! Pruebas de integración del chat: prompt estricto, aislamiento de contexto
//! por carpeta (HU-06), streaming (HU-12) y persistencia de citas (HU-11).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use raggy_core::chat::{ChatWorker, ProviderFactory};
use raggy_core::{compose_request, context_from_retrieved, ChatEvent, IndexOptions};
use raggy_llm::{MockProvider, ProviderId, Role, STRICT_SYSTEM_PROMPT};
use raggy_rag::{ChunkingOptions, RetrieverOptions};
use raggy_storage::{Folder, Message, Session, Storage};

struct Temporary {
    path: PathBuf,
}

impl Temporary {
    fn new(tag: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("raggy-chat-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("crea directorio temporal");
        Self { path }
    }

    fn write(&self, relative: &str, content: &str) {
        std::fs::write(self.path.join(relative), content).expect("escribe archivo");
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn index_options() -> IndexOptions {
    IndexOptions {
        chunking: ChunkingOptions {
            max_chars: 400,
            overlap_chars: 40,
        },
        embedding_dimensions: 128,
        extract: raggy_core::ExtractOptions::default(),
    }
}

fn retriever_options() -> RetrieverOptions {
    RetrieverOptions {
        top_k: 4,
        relative_floor: 0.15,
    }
}

fn register_and_index(storage: &mut Storage, dir: &Temporary, name: &str) -> Folder {
    let folder = Folder::new(dir.path.to_string_lossy().to_string(), name);
    storage.upsert_folder(&folder).expect("guarda carpeta");
    let mut noop = |_| {};
    raggy_core::index_folder(storage, &folder.id, &dir.path, &index_options(), &mut noop)
        .expect("indexa");
    folder
}

#[test]
fn el_prompt_incluye_la_instruccion_estricta_y_la_cita() {
    let docs = Temporary::new("prompt");
    docs.write(
        "guia.md",
        "# Compilación\n\nPara compilar la aplicación ejecuta cargo build.\n",
    );

    let data = Temporary::new("prompt-data");
    let db_path = data.path.join("raggy.sqlite3");
    let mut storage = Storage::open(&db_path).expect("abre la base de datos");
    let folder = register_and_index(&mut storage, &docs, "docs");

    let chunks = raggy_core::search_folder(
        &storage,
        &folder.id,
        "cómo compilar",
        &retriever_options(),
        128,
    )
    .expect("busca");
    let (blocks, citations) = context_from_retrieved(&chunks);
    assert!(!citations.is_empty());

    let request = compose_request("mock", 0.1, 256, &blocks, &[], &[], "¿Cómo compilar?");
    assert_eq!(request.messages[0].role, Role::System);
    assert!(request.messages[0].content.contains(STRICT_SYSTEM_PROMPT));
    assert!(request
        .messages
        .iter()
        .any(|message| message.content.contains("guia.md")));
    assert!(request.messages[0]
        .content
        .contains(&format!("[Fuente: {}]", blocks[0].citation)));
    assert!(request.messages[0]
        .content
        .contains("No inventes ni modifiques"));
    assert_eq!(request.messages.last().expect("último").role, Role::User);
    assert_eq!(request.temperature, 0.1);
}

#[test]
fn el_contexto_esta_aislado_por_carpeta() {
    let docs_a = Temporary::new("iso-a");
    docs_a.write(
        "a.md",
        "# A\n\nEl protocolo FOXTROT solo existe en la carpeta A.\n",
    );
    let docs_b = Temporary::new("iso-b");
    docs_b.write(
        "b.md",
        "# B\n\nEl protocolo ZULU solo existe en la carpeta B.\n",
    );

    let data = Temporary::new("iso-data");
    let db_path = data.path.join("raggy.sqlite3");
    let mut storage = Storage::open(&db_path).expect("abre la base de datos");
    let folder_a = register_and_index(&mut storage, &docs_a, "docs-a");
    let folder_b = register_and_index(&mut storage, &docs_b, "docs-b");

    let results_a = raggy_core::search_folder(
        &storage,
        &folder_a.id,
        "protocolo FOXTROT ZULU",
        &retriever_options(),
        128,
    )
    .expect("busca en A");

    assert!(!results_a.is_empty());
    assert!(results_a
        .iter()
        .all(|result| !result.chunk.content.contains("ZULU")));

    let results_b = raggy_core::search_folder(
        &storage,
        &folder_b.id,
        "protocolo FOXTROT ZULU",
        &retriever_options(),
        128,
    )
    .expect("busca en B");
    assert!(results_b
        .iter()
        .all(|result| !result.chunk.content.contains("FOXTROT")));
}

#[test]
fn el_chat_transmite_y_persiste_la_respuesta_con_citas() {
    let docs = Temporary::new("chat-docs");
    docs.write(
        "instalacion.md",
        "# Instalación\n\nInstala la aplicación con cargo build --release.\n",
    );

    let data = Temporary::new("chat-data");
    let db_path = data.path.join("raggy.sqlite3");

    let (folder, session) = {
        let mut storage = Storage::open(&db_path).expect("abre la base de datos");
        let folder = register_and_index(&mut storage, &docs, "docs");
        let session = Session::new(&folder.id, "deepseek", "mock");
        storage.upsert_session(&session).expect("guarda sesión");
        (folder, session)
    };

    let storage = Storage::open(&db_path).expect("reabre la base de datos");
    let chunks = raggy_core::search_folder(
        &storage,
        &folder.id,
        "cómo instalo la aplicación",
        &retriever_options(),
        128,
    )
    .expect("busca");
    let (blocks, citations) = context_from_retrieved(&chunks);
    let request = compose_request(
        "mock",
        0.1,
        256,
        &blocks,
        &[],
        &[],
        "¿Cómo instalo la aplicación?",
    );

    let reply = format!(
        "Instala la aplicación con cargo build --release. [{}]",
        citations.first().expect("fuente recuperada").label()
    );
    let factory: ProviderFactory =
        Arc::new(move |id| Ok(Box::new(MockProvider::with_reply(id, reply.clone()))));
    let worker = ChatWorker::spawn(db_path.clone(), factory).expect("arranca el chat");

    // Persistimos el mensaje del usuario como hace la interfaz.
    let user_message = Message::new(
        &session.id,
        "user",
        "¿Cómo instalo la aplicación?",
        false,
        vec![],
    );
    storage
        .add_message(&user_message)
        .expect("guarda el mensaje");

    worker.generate(session.id.clone(), ProviderId::DeepSeek, request, citations);

    let mut streamed = String::new();
    let mut finished = false;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && !finished {
        for event in worker.drain() {
            match event {
                ChatEvent::Delta { text, .. } => streamed.push_str(&text),
                ChatEvent::Finished { .. } => finished = true,
                ChatEvent::Failed { message, .. } => panic!("generación fallida: {message}"),
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(finished, "la generación no terminó a tiempo");
    assert!(!streamed.is_empty());

    drop(worker);

    let storage = Storage::open(&db_path).expect("reabre la base de datos");
    let messages = storage.list_messages(&session.id).expect("lista mensajes");
    assert_eq!(messages.len(), 2);

    let assistant = messages.last().expect("respuesta");
    assert_eq!(assistant.role, "assistant");
    assert!(!assistant.content.is_empty());
    assert!(assistant.anchored, "la cita del texto debe verificarse");
    assert!(!assistant.citations.is_empty(), "debería citar fuentes");
    assert!(assistant.citations[0]
        .source_path
        .ends_with("instalacion.md"));
}
