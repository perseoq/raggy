//! Prueba de integración de la memoria de proyecto (HU-16, HU-17, HU-20, HU-21):
//! del bloque de código en la respuesta al archivo en disco, su registro y su
//! presencia en el prompt de la sesión.

use std::path::PathBuf;

use raggy_core::{artifacts_for_prompt, relative_to, save_artifact, ConflictPolicy};
use raggy_llm::extract_code_blocks;
use raggy_storage::{Artifact, Folder, Session, Storage};

struct Temporary {
    path: PathBuf,
}

impl Temporary {
    fn new(tag: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("raggy-artifact-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("crea directorio temporal");
        Self { path }
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn del_bloque_de_codigo_al_artefacto_y_al_prompt() {
    let answer = "Aquí tienes el archivo `saludo.py`:\n\n\
                  ```python title=\"saludo.py\"\nprint('hola')\n```\n";
    let blocks = extract_code_blocks(answer);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].filename.as_deref(), Some("saludo.py"));

    let destination = Temporary::new("destino");
    let outcome = save_artifact(
        &destination.path,
        "saludo.py",
        &blocks[0].content,
        ConflictPolicy::Fail,
    )
    .expect("guarda el artefacto");
    let path = outcome.path().to_path_buf();
    assert_eq!(
        std::fs::read_to_string(&path).expect("lee"),
        "print('hola')"
    );

    let storage = Storage::open_in_memory().expect("abre la base de datos");
    let folder = Folder::new("/tmp/docs", "docs");
    storage.upsert_folder(&folder).expect("guarda carpeta");
    let session = Session::new(&folder.id, "deepseek", "mock");
    storage.upsert_session(&session).expect("guarda sesión");

    let relative = relative_to(&destination.path, &path);
    let artifact = Artifact::new(
        &session.id,
        "saludo.py",
        relative.clone(),
        path.to_string_lossy().to_string(),
        blocks[0].language.clone(),
        &blocks[0].content,
        Some("mock".to_string()),
    );
    storage.add_artifact(&artifact).expect("registra artefacto");

    let stored = storage
        .list_artifacts_by_session(&session.id)
        .expect("lista por sesión");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].relative_path, "saludo.py");

    let by_folder = storage
        .list_artifacts_by_folder(&folder.id)
        .expect("lista por carpeta");
    assert_eq!(by_folder.len(), 1);

    // Si el usuario lo menciona, se incluye su contenido en el prompt (HU-21).
    let contexts = artifacts_for_prompt(&stored, "cambia saludo.py");
    assert_eq!(contexts.len(), 1);
    assert!(contexts[0]
        .content
        .as_deref()
        .is_some_and(|c| c.contains("print")));

    // Al archivarlo deja de ofrecerse al modelo.
    storage
        .set_artifact_archived(&artifact.id, true)
        .expect("archiva");
    let stored = storage
        .list_artifacts_by_session(&session.id)
        .expect("lista");
    assert!(stored[0].archived);
    assert!(artifacts_for_prompt(&stored, "saludo.py").is_empty());

    // Eliminar el registro no borra el archivo en disco.
    storage
        .delete_artifact(&artifact.id)
        .expect("elimina registro");
    assert!(storage
        .list_artifacts_by_session(&session.id)
        .expect("lista")
        .is_empty());
    assert!(path.exists(), "el archivo físico no debería borrarse");
}
