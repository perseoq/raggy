//! Regresión (fase 1): preguntas naturales deben recuperar el documento correcto
//! con la configuración por defecto. Antes, un corte absoluto de 0.3 descartaba
//! el mejor resultado cuando su puntuación era baja.

use std::path::PathBuf;

use raggy_core::IndexOptions;
use raggy_rag::RetrieverOptions;
use raggy_storage::{Folder, Storage};

struct Temporal {
    path: PathBuf,
}

impl Temporal {
    fn new(tag: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("raggy-recuperacion-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("crea directorio temporal");
        Self { path }
    }

    fn write(&self, relative: &str, content: &str) {
        std::fs::write(self.path.join(relative), content).expect("escribe archivo");
    }
}

impl Drop for Temporal {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// (archivo, contenido) con temas claramente distintos.
const DOCUMENTOS: &[(&str, &str)] = &[
    ("facturacion.md", "# Facturación y pagos\n\nLa cuota mensual del plan empresarial es de 49 euros por usuario. El cobro se domicilia por transferencia bancaria el día 5 de cada mes.\n"),
    ("despliegue.md", "# Despliegue en producción\n\nPara publicar una versión nueva se compila en modo release y se copia el binario al servidor. El proceso dura unos diez minutos.\n"),
    ("vacaciones.md", "# Política de vacaciones\n\nCada persona dispone de 23 días laborables de descanso al año. Las solicitudes se envían con dos semanas de antelación.\n"),
    ("red.md", "# Configuración de red\n\nEl servicio escucha en el puerto 8080 por defecto. Para cambiarlo, edita el fichero settings.json y reinicia con systemctl restart raggy.\n"),
];

/// (pregunta natural, archivo que debe responderla)
const PREGUNTAS: &[(&str, &str)] = &[
    (
        "¿cuánto cuesta el plan para empresas cada mes?",
        "facturacion.md",
    ),
    (
        "¿cómo publico una versión nueva en el servidor?",
        "despliegue.md",
    ),
    ("¿cuántos días de descanso tengo al año?", "vacaciones.md"),
    ("¿en qué puerto escucha la aplicación?", "red.md"),
];

#[test]
fn las_preguntas_naturales_recuperan_su_documento() {
    let docs = Temporal::new("docs");
    for (name, content) in DOCUMENTOS {
        docs.write(name, content);
    }
    // Relleno para que no sea un corpus trivial.
    for index in 1..=20 {
        docs.write(
            &format!("notas_{index}.md"),
            &format!("# Notas {index}\n\nApuntes varios sobre el sistema y los procedimientos internos del equipo número {index}.\n"),
        );
    }

    let data = Temporal::new("data");
    let db_path = data.path.join("raggy.sqlite3");

    let folder = {
        let storage = Storage::open(&db_path).expect("abre la base de datos");
        let folder = Folder::new(docs.path.to_string_lossy().to_string(), "docs");
        storage.upsert_folder(&folder).expect("registra la carpeta");
        folder
    };

    let mut storage = Storage::open(&db_path).expect("reabre la base de datos");
    let mut noop = |_| {};
    raggy_core::index_folder(
        &mut storage,
        &folder.id,
        &docs.path,
        &IndexOptions::default(),
        &mut noop,
    )
    .expect("indexa");

    // Configuración por defecto, la misma que usa el chat.
    let options = RetrieverOptions::default();

    for (pregunta, esperado) in PREGUNTAS {
        let results = raggy_core::search_folder(&storage, &folder.id, pregunta, &options, 512)
            .expect("busca");

        assert!(
            !results.is_empty(),
            "«{pregunta}» no recuperó ningún fragmento"
        );
        assert_eq!(
            results[0].chunk.metadata.source_path, *esperado,
            "«{pregunta}» debería responder con {esperado}, pero el primero fue {}",
            results[0].chunk.metadata.source_path
        );
    }
}
