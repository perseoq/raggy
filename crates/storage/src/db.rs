//! Apertura de la base de datos y gestión del esquema.

use std::path::Path;

use rusqlite::Connection;

use crate::error::{Result, StorageError};
use crate::migrations;

/// Manejador de la base de datos local.
///
/// Mantiene una única conexión. SQLite es síncrono y `Connection` no es `Send`,
/// por lo que el acceso se realiza siempre desde el hilo principal o desde un
/// hilo dedicado que garantice la exclusividad.
pub struct Storage {
    conn: Connection,
}

impl Storage {
    /// Abre (o crea) la base de datos en `path` y aplica las migraciones.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| StorageError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let conn = Connection::open(path)?;
        Self::from_connection(conn)
    }

    /// Abre una base de datos efímera en memoria, útil para pruebas.
    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    /// Configura los pragmas recomendados antes de aplicar migraciones.
    fn configure(conn: &Connection) -> Result<()> {
        // La UI, el indexador y el chat tienen conexiones independientes; dar
        // tiempo a SQLite para que otra escritura termine en lugar de fallar
        // inmediatamente con SQLITE_BUSY.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // `journal_mode` devuelve una fila con el modo resultante.
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL;")?;
        Ok(())
    }

    fn from_connection(mut conn: Connection) -> Result<Self> {
        Self::configure(&conn)?;
        migrations::apply(&mut conn)?;
        Ok(Self { conn })
    }

    /// Acceso de solo lectura a la conexión subyacente.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Acceso mutable a la conexión subyacente.
    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Versión de esquema actualmente aplicada.
    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    /// Escribe una copia consistente de la base de datos en `path` (HU-32).
    pub fn backup_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| StorageError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let mut destination = Connection::open(path)?;
        let backup = rusqlite::backup::Backup::new(&self.conn, &mut destination)?;
        backup.run_to_completion(16, std::time::Duration::from_millis(10), None)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Document, Folder, FolderStatus, IndexOwner, Session};

    #[test]
    fn marcar_indexando_no_cambia_la_revision_de_la_carpeta() {
        let storage = Storage::open_in_memory().expect("abre en memoria");
        let folder = Folder::new("/tmp/raggy-revision", "revision");
        storage.upsert_folder(&folder).expect("guarda carpeta");
        let before = storage
            .get_folder(&folder.id)
            .expect("lee carpeta")
            .expect("existe")
            .updated_at;

        storage.mark_folder_indexing(&folder.id).expect("marca");
        let during = storage
            .get_folder(&folder.id)
            .expect("lee carpeta")
            .expect("existe");
        assert_eq!(
            during.updated_at, before,
            "indexar no debe invalidar la caché de búsqueda"
        );
        assert_eq!(during.status, FolderStatus::Indexing);

        storage
            .set_folder_status(&folder.id, FolderStatus::Indexed, 3)
            .expect("termina");
        let after = storage
            .get_folder(&folder.id)
            .expect("lee carpeta")
            .expect("existe");
        assert_ne!(
            after.updated_at, before,
            "al terminar sí avanza la revisión"
        );
    }

    #[test]
    fn reemplaza_chunks_y_documento_en_una_sola_operacion() {
        let mut storage = Storage::open_in_memory().expect("abre en memoria");
        let folder = Folder::new("/tmp/raggy-fichero", "fichero");
        storage.upsert_folder(&folder).expect("guarda carpeta");

        let chunk = raggy_rag::Chunk {
            id: "c1".to_string(),
            content_hash: "h".to_string(),
            content: "contenido".to_string(),
            metadata: raggy_rag::ChunkMetadata {
                source_file: "a.md".to_string(),
                source_path: "a.md".to_string(),
                heading: None,
                location: None,
                start_line: 1,
                end_line: 1,
            },
        };
        let document = Document::new(&folder.id, "a.md", "h", "p", 9, 0, 1);
        storage
            .replace_file_index(
                IndexOwner::Folder(&folder.id),
                "a.md",
                &[chunk],
                &[vec![0.0; 8]],
                &document,
            )
            .expect("escribe el índice");

        assert_eq!(storage.count_chunks(&folder.id).expect("cuenta"), 1);
        let documents = storage.list_documents(&folder.id).expect("lista");
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].chunk_count, 1);

        // Un archivo sin texto deja el documento y sus chunks coherentes a cero.
        let vacio = Document::new(&folder.id, "a.md", "h2", "p2", 0, 0, 0);
        storage
            .replace_file_index(IndexOwner::Folder(&folder.id), "a.md", &[], &[], &vacio)
            .expect("reemplaza");
        assert_eq!(storage.count_chunks(&folder.id).expect("cuenta"), 0);
        let documents = storage.list_documents(&folder.id).expect("lista");
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].chunk_count, 0);
    }

    #[test]
    fn las_migraciones_dejan_la_version_esperada() {
        let storage = Storage::open_in_memory().expect("abre en memoria");
        assert_eq!(
            storage.schema_version().expect("lee versión"),
            migrations::LATEST_SCHEMA_VERSION
        );
    }

    #[test]
    fn configura_un_tiempo_de_espera_para_escrituras_concurrentes() {
        let storage = Storage::open_in_memory().expect("abre en memoria");
        let timeout: i64 = storage
            .connection()
            .pragma_query_value(None, "busy_timeout", |row| row.get(0))
            .expect("lee busy_timeout");
        assert_eq!(timeout, 5_000);
    }

    #[test]
    fn se_crean_todas_las_tablas_del_esquema() {
        let storage = Storage::open_in_memory().expect("abre en memoria");
        for table in [
            "folders",
            "documents",
            "chunks",
            "sessions",
            "messages",
            "artifacts",
        ] {
            let count: i64 = storage
                .connection()
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .expect("consulta sqlite_master");
            assert_eq!(count, 1, "falta la tabla `{table}`");
        }
    }

    #[test]
    fn documentos_persisten_la_huella_del_pipeline() {
        let storage = Storage::open_in_memory().expect("abre en memoria");
        let count: i64 = storage
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('documents') WHERE name = 'processing_hash'",
                [],
                |row| row.get(0),
            )
            .expect("consulta columnas");
        assert_eq!(count, 1);
    }

    #[test]
    fn la_clave_foranea_en_cascada_elimina_dependencias() {
        let storage = Storage::open_in_memory().expect("abre en memoria");
        let conn = storage.connection();

        conn.execute(
            "INSERT INTO folders (id, path, name, status, file_count, created_at, updated_at)
             VALUES ('f1', '/tmp/docs', 'docs', 'indexed', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        )
        .expect("inserta carpeta");

        conn.execute(
            "INSERT INTO sessions (id, folder_id, title, provider, model, created_at, updated_at)
             VALUES ('s1', 'f1', 'prueba', 'deepseek', 'deepseek-chat', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        )
        .expect("inserta sesión");

        conn.execute("DELETE FROM folders WHERE id = 'f1'", [])
            .expect("borra carpeta");

        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .expect("cuenta sesiones");
        assert_eq!(remaining, 0, "la sesión debió eliminarse en cascada");
    }

    #[test]
    fn al_leer_mensajes_legacy_revalida_el_anclaje_y_las_fuentes() {
        let storage = Storage::open_in_memory().expect("abre en memoria");
        let connection = storage.connection();
        connection
            .execute(
                "INSERT INTO folders (id, path, name, status, file_count, created_at, updated_at)
                 VALUES ('f1', '/tmp/docs', 'docs', 'indexed', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .expect("inserta carpeta");
        connection
            .execute(
                "INSERT INTO sessions (id, folder_id, title, provider, model, created_at, updated_at)
                 VALUES ('s1', 'f1', 'prueba', 'deepseek', 'deepseek-chat', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .expect("inserta sesión");

        let candidate = raggy_rag::Citation {
            source_file: "guia.md".to_string(),
            source_path: "docs/guia.md".to_string(),
            heading: None,
            location: Some("página 2".to_string()),
            start_line: 4,
            end_line: 8,
            score: 0.9,
        };
        let sources = serde_json::to_string(&[candidate]).expect("serializa las fuentes");
        connection
            .execute(
                "INSERT INTO messages (id, session_id, role, content, anchored, citations, created_at)
                 VALUES ('m1', 's1', 'assistant', 'respuesta sin referencia', 1, ?1, '2026-01-01T00:00:00Z')",
                [sources],
            )
            .expect("inserta mensaje antiguo inconsistente");

        let message = storage
            .list_messages("s1")
            .expect("lee el mensaje")
            .remove(0);
        assert!(!message.anchored);
        assert!(message.citations.is_empty());
    }

    fn fragmento(id: &str, content: &str) -> raggy_rag::Chunk {
        raggy_rag::Chunk {
            id: id.to_string(),
            content_hash: id.to_string(),
            content: content.to_string(),
            metadata: raggy_rag::ChunkMetadata {
                source_file: "informe.pdf".to_string(),
                source_path: "informe.pdf".to_string(),
                heading: None,
                location: None,
                start_line: 1,
                end_line: 1,
            },
        }
    }

    #[test]
    fn la_migracion_de_adjuntos_conserva_los_datos_existentes() {
        // Se reconstruye la base de datos hasta la versión 8 y se comprueba que
        // la migración a la 9 conserva carpetas, documentos y fragmentos.
        let mut conn = Connection::open_in_memory().expect("abre en memoria");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("activa claves foráneas");
        for migration in migrations::MIGRATIONS.iter().filter(|m| m.version <= 8) {
            let tx = conn.transaction().expect("abre transacción");
            tx.execute_batch(migration.sql)
                .expect("aplica migración antigua");
            tx.pragma_update(None, "user_version", migration.version)
                .expect("marca la versión");
            tx.commit().expect("confirma la migración");
        }
        conn.execute(
            "INSERT INTO folders (id, path, name, status, file_count, created_at, updated_at)
             VALUES ('f1', '/tmp/legacy', 'legacy', 'indexed', 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        )
        .expect("inserta carpeta");
        conn.execute(
            "INSERT INTO chunks (id, folder_id, source_path, source_file, start_line, end_line, content_hash, content)
             VALUES ('c1', 'f1', 'a.md', 'a.md', 1, 3, 'h', 'texto heredado')",
            [],
        )
        .expect("inserta fragmento");
        conn.execute(
            "INSERT INTO documents (id, folder_id, relative_path, content_hash, size_bytes, modified_at, indexed_at, chunk_count)
             VALUES ('d1', 'f1', 'a.md', 'h', 13, 0, '2026-01-01T00:00:00Z', 1)",
            [],
        )
        .expect("inserta documento");

        let version = migrations::apply(&mut conn).expect("aplica las migraciones pendientes");
        assert_eq!(version, migrations::LATEST_SCHEMA_VERSION);

        let chunks: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE folder_id = 'f1' AND session_id IS NULL",
                [],
                |row| row.get(0),
            )
            .expect("cuenta fragmentos");
        assert_eq!(
            chunks, 1,
            "los fragmentos existentes se conservan sin sesión"
        );
        let documents: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM documents WHERE folder_id = 'f1' AND session_id IS NULL",
                [],
                |row| row.get(0),
            )
            .expect("cuenta documentos");
        assert_eq!(documents, 1, "los documentos existentes se conservan");
    }

    #[test]
    fn un_registro_debe_pertenecer_a_una_carpeta_o_a_una_sesion() {
        let storage = Storage::open_in_memory().expect("abre en memoria");
        let conn = storage.connection();
        conn.execute(
            "INSERT INTO folders (id, path, name, status, file_count, created_at, updated_at)
             VALUES ('f1', '/tmp/propietario', 'propietario', 'indexed', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        )
        .expect("inserta carpeta");
        conn.execute(
            "INSERT INTO sessions (id, folder_id, title, provider, model, created_at, updated_at)
             VALUES ('s1', 'f1', 't', 'deepseek', 'm', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        )
        .expect("inserta sesión");

        // Sin propietario: se rechaza.
        assert!(
            conn.execute(
                "INSERT INTO chunks (id, source_path, source_file, start_line, end_line, content_hash, content)
                 VALUES ('c0', 'a.md', 'a.md', 1, 1, 'h', 'x')",
                [],
            )
            .is_err(),
            "un fragmento sin propietario no debe admitirse"
        );

        // Con los dos propietarios: también se rechaza.
        assert!(
            conn.execute(
                "INSERT INTO chunks (id, folder_id, session_id, source_path, source_file, start_line, end_line, content_hash, content)
                 VALUES ('c2', 'f1', 's1', 'a.md', 'a.md', 1, 1, 'h', 'x')",
                [],
            )
            .is_err(),
            "un fragmento no puede tener dos propietarios"
        );

        // Con uno solo: se admite.
        conn.execute(
            "INSERT INTO chunks (id, session_id, source_path, source_file, start_line, end_line, content_hash, content)
             VALUES ('c3', 's1', 'informe.pdf', 'informe.pdf', 1, 1, 'h', 'x')",
            [],
        )
        .expect("un adjunto con un solo propietario es válido");
    }

    #[test]
    fn guarda_adjuntos_y_los_elimina_en_cascada_con_la_sesion() {
        let mut storage = Storage::open_in_memory().expect("abre en memoria");
        let folder = Folder::new("/tmp/cascada", "cascada");
        storage.upsert_folder(&folder).expect("guarda carpeta");
        let session = Session::new(&folder.id, "deepseek", "deepseek-chat");
        storage.upsert_session(&session).expect("guarda sesión");

        let document = Document::for_session(&session.id, "informe.pdf", "h", "p", 10, 0, 1);
        storage
            .replace_file_index(
                IndexOwner::Session(&session.id),
                "informe.pdf",
                &[fragmento("c1", "contenido del adjunto")],
                &[vec![0.0; 8]],
                &document,
            )
            .expect("guarda el adjunto");

        let adjuntos = storage.list_session_documents(&session.id).expect("lista");
        assert_eq!(adjuntos.len(), 1);
        assert_eq!(adjuntos[0].session_id.as_deref(), Some(session.id.as_str()));
        assert!(
            storage
                .list_embedded_chunks_for_session(&session.id)
                .expect("fragmentos")
                .len()
                == 1
        );
        assert!(storage
            .session_index_revision(&session.id)
            .expect("revisión")
            .starts_with('1'));

        storage.delete_session(&session.id).expect("borra sesión");
        assert!(storage
            .list_session_documents(&session.id)
            .expect("lista")
            .is_empty());
        let chunks: i64 = storage
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE session_id = ?1",
                [&session.id],
                |row| row.get(0),
            )
            .expect("cuenta fragmentos");
        assert_eq!(chunks, 0, "los fragmentos se borran en cascada");
    }

    #[test]
    fn quitar_los_adjuntos_de_una_sesion_no_toca_la_carpeta() {
        let mut storage = Storage::open_in_memory().expect("abre en memoria");
        let folder = Folder::new("/tmp/adjuntos", "adjuntos");
        storage.upsert_folder(&folder).expect("guarda carpeta");
        let session = Session::new(&folder.id, "deepseek", "deepseek-chat");
        storage.upsert_session(&session).expect("guarda sesión");

        storage
            .replace_file_index(
                IndexOwner::Folder(&folder.id),
                "guia.md",
                &[fragmento("carpeta", "índice de la carpeta")],
                &[vec![0.0; 8]],
                &Document::new(&folder.id, "guia.md", "h", "p", 10, 0, 1),
            )
            .expect("guarda la carpeta");
        storage
            .replace_file_index(
                IndexOwner::Session(&session.id),
                "informe.pdf",
                &[fragmento("adjunto", "índice del adjunto")],
                &[vec![0.0; 8]],
                &Document::for_session(&session.id, "informe.pdf", "h", "p", 10, 0, 1),
            )
            .expect("guarda el adjunto");

        storage
            .delete_session_index(&session.id)
            .expect("quita los adjuntos");

        assert!(storage
            .list_session_documents(&session.id)
            .expect("adjuntos")
            .is_empty());
        assert_eq!(
            storage
                .list_documents(&folder.id)
                .expect("documentos")
                .len(),
            1,
            "el índice de la carpeta no se altera"
        );
        assert_eq!(storage.count_chunks(&folder.id).expect("cuenta"), 1);
    }

    #[test]
    fn recupera_los_fragmentos_de_un_adjunto_por_su_ruta() {
        let mut storage = Storage::open_in_memory().expect("abre en memoria");
        let folder = Folder::new("/tmp/citas", "citas");
        storage.upsert_folder(&folder).expect("guarda carpeta");
        let session = Session::new(&folder.id, "deepseek", "deepseek-chat");
        storage.upsert_session(&session).expect("guarda sesión");

        // El mismo nombre de archivo existe en la carpeta y como adjunto: cada
        // propietario debe ver solo su propio fragmento (HU-34).
        storage
            .replace_file_index(
                IndexOwner::Folder(&folder.id),
                "informe.pdf",
                &[fragmento("carpeta", "fragmento de la carpeta")],
                &[vec![0.0; 8]],
                &Document::new(&folder.id, "informe.pdf", "h", "p", 10, 0, 1),
            )
            .expect("guarda la carpeta");
        storage
            .replace_file_index(
                IndexOwner::Session(&session.id),
                "informe.pdf",
                &[fragmento("adjunto", "fragmento del adjunto")],
                &[vec![0.0; 8]],
                &Document::for_session(&session.id, "informe.pdf", "h", "p", 10, 0, 1),
            )
            .expect("guarda el adjunto");

        let del_adjunto = storage
            .list_session_chunks_for_path(&session.id, "informe.pdf")
            .expect("fragmentos del adjunto");
        assert_eq!(del_adjunto.len(), 1);
        assert_eq!(del_adjunto[0].content, "fragmento del adjunto");

        let de_la_carpeta = storage
            .list_chunks_for_path(&folder.id, "informe.pdf")
            .expect("fragmentos de la carpeta");
        assert_eq!(de_la_carpeta.len(), 1);
        assert_eq!(de_la_carpeta[0].content, "fragmento de la carpeta");

        assert!(storage
            .list_session_chunks_for_path(&session.id, "otro.pdf")
            .expect("ruta inexistente")
            .is_empty());
    }
}
