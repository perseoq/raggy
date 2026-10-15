//! Consultas y mutaciones sobre SQLite para las entidades del dominio.

use rusqlite::{params, OptionalExtension, Row};

use raggy_rag::Chunk;

use crate::error::Result;
use crate::models::{Artifact, Document, Folder, FolderStatus, Message, Session};
use crate::Storage;

impl Storage {
    // ----------------------------------------------------------------- carpetas

    /// Inserta o actualiza una carpeta.
    pub fn upsert_folder(&self, folder: &Folder) -> Result<()> {
        self.connection().execute(
            "INSERT INTO folders (id, path, name, status, file_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                 path = excluded.path,
                 name = excluded.name,
                 status = excluded.status,
                 file_count = excluded.file_count,
                 updated_at = excluded.updated_at",
            params![
                folder.id,
                folder.path,
                folder.name,
                folder.status.as_str(),
                folder.file_count,
                folder.created_at,
                folder.updated_at,
            ],
        )?;
        Ok(())
    }

    /// Lista todas las carpetas por fecha de creación.
    pub fn list_folders(&self) -> Result<Vec<Folder>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, path, name, status, file_count, created_at, updated_at
             FROM folders ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], row_to_folder)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Busca una carpeta por su ruta absoluta.
    pub fn find_folder_by_path(&self, path: &str) -> Result<Option<Folder>> {
        let folder = self
            .connection()
            .query_row(
                "SELECT id, path, name, status, file_count, created_at, updated_at
                 FROM folders WHERE path = ?1",
                [path],
                row_to_folder,
            )
            .optional()?;
        Ok(folder)
    }

    /// Recupera una carpeta por identificador.
    pub fn get_folder(&self, id: &str) -> Result<Option<Folder>> {
        let folder = self
            .connection()
            .query_row(
                "SELECT id, path, name, status, file_count, created_at, updated_at
                 FROM folders WHERE id = ?1",
                [id],
                row_to_folder,
            )
            .optional()?;
        Ok(folder)
    }

    /// Actualiza la ruta de una carpeta gestionada al restaurar datos en otra
    /// instalación.
    pub fn update_folder_path(&self, id: &str, path: &str) -> Result<()> {
        self.connection().execute(
            "UPDATE folders SET path = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, path, now_rfc3339()],
        )?;
        Ok(())
    }

    /// Actualiza el estado y el contador de archivos de una carpeta.
    pub fn set_folder_status(&self, id: &str, status: FolderStatus, file_count: i64) -> Result<()> {
        self.connection().execute(
            "UPDATE folders SET status = ?2, file_count = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, status.as_str(), file_count, now_rfc3339()],
        )?;
        Ok(())
    }

    /// Marca la carpeta como «en indexación» sin cambiar su fecha de revisión.
    ///
    /// La fecha solo avanza cuando la indexación termina, de modo que la caché de
    /// búsqueda no se invalida a mitad de proceso (evita reconstruir el
    /// recuperador con datos parciales y luego otra vez).
    pub fn mark_folder_indexing(&self, id: &str) -> Result<()> {
        self.connection().execute(
            "UPDATE folders SET status = ?2, file_count = 0 WHERE id = ?1",
            params![id, FolderStatus::Indexing.as_str()],
        )?;
        Ok(())
    }

    /// Elimina una carpeta y, en cascada, sus documentos y chunks.
    pub fn delete_folder(&self, id: &str) -> Result<()> {
        self.connection()
            .execute("DELETE FROM folders WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---------------------------------------------------------------- documentos

    /// Lista los documentos de una carpeta.
    pub fn list_documents(&self, folder_id: &str) -> Result<Vec<Document>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, folder_id, relative_path, content_hash, processing_hash, size_bytes,
                    modified_at, indexed_at, chunk_count
             FROM documents WHERE folder_id = ?1 ORDER BY relative_path ASC",
        )?;
        let rows = stmt.query_map([folder_id], row_to_document)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Recupera un documento por carpeta y ruta relativa.
    pub fn get_document(&self, folder_id: &str, relative_path: &str) -> Result<Option<Document>> {
        let document = self
            .connection()
            .query_row(
                "SELECT id, folder_id, relative_path, content_hash, processing_hash, size_bytes,
                        modified_at, indexed_at, chunk_count
                 FROM documents WHERE folder_id = ?1 AND relative_path = ?2",
                params![folder_id, relative_path],
                row_to_document,
            )
            .optional()?;
        Ok(document)
    }

    /// Inserta o actualiza el registro de un documento.
    pub fn upsert_document(&self, document: &Document) -> Result<()> {
        self.connection().execute(
            "INSERT INTO documents
                 (id, folder_id, relative_path, content_hash, processing_hash, size_bytes,
                  modified_at, indexed_at, chunk_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(folder_id, relative_path) DO UPDATE SET
                  content_hash = excluded.content_hash,
                  processing_hash = excluded.processing_hash,
                  size_bytes = excluded.size_bytes,
                 modified_at = excluded.modified_at,
                 indexed_at = excluded.indexed_at,
                 chunk_count = excluded.chunk_count",
            params![
                document.id,
                document.folder_id,
                document.relative_path,
                document.content_hash,
                document.processing_hash,
                document.size_bytes,
                document.modified_at,
                document.indexed_at,
                document.chunk_count,
            ],
        )?;
        Ok(())
    }

    /// Elimina el registro de un documento (sin tocar los chunks).
    pub fn delete_document(&self, folder_id: &str, relative_path: &str) -> Result<()> {
        self.connection().execute(
            "DELETE FROM documents WHERE folder_id = ?1 AND relative_path = ?2",
            params![folder_id, relative_path],
        )?;
        Ok(())
    }

    // -------------------------------------------------------------------- chunks

    /// Borra los chunks asociados a un archivo.
    pub fn delete_chunks_for_path(&self, folder_id: &str, source_path: &str) -> Result<()> {
        self.connection().execute(
            "DELETE FROM chunks WHERE folder_id = ?1 AND source_path = ?2",
            params![folder_id, source_path],
        )?;
        Ok(())
    }

    /// Reemplaza de forma atómica los chunks de un archivo y sus embeddings.
    ///
    /// `embeddings` debe tener la misma longitud que `chunks`.
    pub fn replace_chunks(
        &mut self,
        folder_id: &str,
        source_path: &str,
        chunks: &[Chunk],
        embeddings: &[Vec<f32>],
    ) -> Result<()> {
        let tx = self.connection_mut().transaction()?;
        tx.execute(
            "DELETE FROM chunks WHERE folder_id = ?1 AND source_path = ?2",
            params![folder_id, source_path],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO chunks
                     (id, folder_id, source_path, source_file, heading, location, start_line,
                      end_line, content_hash, content, embedding)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?;
            for (chunk, embedding) in chunks.iter().zip(embeddings) {
                stmt.execute(params![
                    chunk.id,
                    folder_id,
                    source_path,
                    chunk.metadata.source_file,
                    chunk.metadata.heading,
                    chunk.metadata.location,
                    chunk.metadata.start_line as i64,
                    chunk.metadata.end_line as i64,
                    chunk.content_hash,
                    chunk.content,
                    encode_vector(embedding),
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Reemplaza los chunks de un archivo **y** su registro de documento en una
    /// sola transacción.
    ///
    /// Así no puede quedar un documento apuntando a un índice antiguo (ni al
    /// revés) si el proceso se interrumpe a mitad de archivo.
    pub fn replace_file_index(
        &mut self,
        folder_id: &str,
        source_path: &str,
        chunks: &[Chunk],
        embeddings: &[Vec<f32>],
        document: &Document,
    ) -> Result<()> {
        let tx = self.connection_mut().transaction()?;
        tx.execute(
            "DELETE FROM chunks WHERE folder_id = ?1 AND source_path = ?2",
            params![folder_id, source_path],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO chunks
                     (id, folder_id, source_path, source_file, heading, location, start_line,
                      end_line, content_hash, content, embedding)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?;
            for (chunk, embedding) in chunks.iter().zip(embeddings) {
                stmt.execute(params![
                    chunk.id,
                    folder_id,
                    source_path,
                    chunk.metadata.source_file,
                    chunk.metadata.heading,
                    chunk.metadata.location,
                    chunk.metadata.start_line as i64,
                    chunk.metadata.end_line as i64,
                    chunk.content_hash,
                    chunk.content,
                    encode_vector(embedding),
                ])?;
            }
        }
        tx.execute(
            "INSERT INTO documents
                 (id, folder_id, relative_path, content_hash, processing_hash, size_bytes,
                  modified_at, indexed_at, chunk_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(folder_id, relative_path) DO UPDATE SET
                  content_hash = excluded.content_hash,
                  processing_hash = excluded.processing_hash,
                  size_bytes = excluded.size_bytes,
                  modified_at = excluded.modified_at,
                  indexed_at = excluded.indexed_at,
                  chunk_count = excluded.chunk_count",
            params![
                document.id,
                document.folder_id,
                document.relative_path,
                document.content_hash,
                document.processing_hash,
                document.size_bytes,
                document.modified_at,
                document.indexed_at,
                document.chunk_count,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Cuenta los chunks de una carpeta.
    pub fn count_chunks(&self, folder_id: &str) -> Result<i64> {
        let count = self.connection().query_row(
            "SELECT COUNT(*) FROM chunks WHERE folder_id = ?1",
            [folder_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Cuenta los chunks con embedding almacenado.
    pub fn count_embedded_chunks(&self, folder_id: &str) -> Result<i64> {
        let count = self.connection().query_row(
            "SELECT COUNT(*) FROM chunks WHERE folder_id = ?1 AND embedding IS NOT NULL",
            [folder_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Cuenta los chunks cuyo embedding tiene la dimensionalidad indicada.
    pub fn count_chunks_with_embedding_dimensions(
        &self,
        folder_id: &str,
        dimensions: usize,
    ) -> Result<i64> {
        let bytes = (dimensions * 4) as i64;
        let count = self.connection().query_row(
            "SELECT COUNT(*) FROM chunks WHERE folder_id = ?1 AND length(embedding) = ?2",
            params![folder_id, bytes],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Recupera todos los chunks de una carpeta con su embedding (si lo hay).
    pub fn list_embedded_chunks(&self, folder_id: &str) -> Result<Vec<(Chunk, Option<Vec<f32>>)>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, source_path, source_file, heading, location, start_line, end_line,
                    content_hash, content, embedding
             FROM chunks WHERE folder_id = ?1 ORDER BY source_path ASC, start_line ASC",
        )?;
        let rows = stmt.query_map([folder_id], |row| {
            let blob: Option<Vec<u8>> = row.get(9)?;
            Ok((
                Chunk {
                    id: row.get(0)?,
                    content_hash: row.get(7)?,
                    content: row.get(8)?,
                    metadata: raggy_rag::ChunkMetadata {
                        source_path: row.get(1)?,
                        source_file: row.get(2)?,
                        heading: row.get(3)?,
                        location: row.get(4)?,
                        start_line: row.get::<_, i64>(5)? as usize,
                        end_line: row.get::<_, i64>(6)? as usize,
                    },
                },
                blob.map(decode_vector),
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Recupera los chunks de un archivo concreto, ordenados por posición.
    pub fn list_chunks_for_path(&self, folder_id: &str, source_path: &str) -> Result<Vec<Chunk>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, source_path, source_file, heading, location, start_line, end_line,
                    content_hash, content
             FROM chunks WHERE folder_id = ?1 AND source_path = ?2 ORDER BY start_line ASC",
        )?;
        let rows = stmt.query_map(params![folder_id, source_path], |row| {
            Ok(Chunk {
                id: row.get(0)?,
                content_hash: row.get(7)?,
                content: row.get(8)?,
                metadata: raggy_rag::ChunkMetadata {
                    source_path: row.get(1)?,
                    source_file: row.get(2)?,
                    heading: row.get(3)?,
                    location: row.get(4)?,
                    start_line: row.get::<_, i64>(5)? as usize,
                    end_line: row.get::<_, i64>(6)? as usize,
                },
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

impl Storage {
    // ---------------------------------------------------------------- sesiones

    /// Inserta o actualiza una sesión.
    pub fn upsert_session(&self, session: &Session) -> Result<()> {
        self.connection().execute(
            "INSERT INTO sessions
                 (id, folder_id, title, provider, model, fallback_provider, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                 title = excluded.title,
                 provider = excluded.provider,
                 model = excluded.model,
                 fallback_provider = excluded.fallback_provider,
                 updated_at = excluded.updated_at",
            params![
                session.id,
                session.folder_id,
                session.title,
                session.provider,
                session.model,
                session.fallback_provider,
                session.created_at,
                session.updated_at,
            ],
        )?;
        Ok(())
    }

    /// Lista las sesiones de una carpeta, las más recientes primero.
    pub fn list_sessions(&self, folder_id: &str) -> Result<Vec<Session>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, folder_id, title, provider, model, fallback_provider, created_at, updated_at
             FROM sessions WHERE folder_id = ?1 ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([folder_id], row_to_session)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Recupera una sesión por identificador.
    pub fn get_session(&self, id: &str) -> Result<Option<Session>> {
        let session = self
            .connection()
            .query_row(
                "SELECT id, folder_id, title, provider, model, fallback_provider, created_at, updated_at
                 FROM sessions WHERE id = ?1",
                [id],
                row_to_session,
            )
            .optional()?;
        Ok(session)
    }

    /// Renombra una sesión.
    pub fn rename_session(&self, id: &str, title: &str) -> Result<()> {
        self.connection().execute(
            "UPDATE sessions SET title = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, title, now_rfc3339()],
        )?;
        Ok(())
    }

    /// Actualiza el proveedor y el modelo de una sesión (HU-15).
    pub fn update_session_model(&self, id: &str, provider: &str, model: &str) -> Result<()> {
        self.connection().execute(
            "UPDATE sessions SET provider = ?2, model = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, provider, model, now_rfc3339()],
        )?;
        Ok(())
    }

    /// Define el proveedor de respaldo de una sesión (HU-25).
    pub fn set_session_fallback(&self, id: &str, fallback: Option<&str>) -> Result<()> {
        self.connection().execute(
            "UPDATE sessions SET fallback_provider = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, fallback, now_rfc3339()],
        )?;
        Ok(())
    }

    /// Marca la sesión como actualizada.
    pub fn touch_session(&self, id: &str) -> Result<()> {
        self.connection().execute(
            "UPDATE sessions SET updated_at = ?2 WHERE id = ?1",
            params![id, now_rfc3339()],
        )?;
        Ok(())
    }

    /// Elimina una sesión y, en cascada, sus mensajes.
    pub fn delete_session(&self, id: &str) -> Result<()> {
        self.connection()
            .execute("DELETE FROM sessions WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---------------------------------------------------------------- mensajes

    /// Añade un mensaje a una sesión.
    pub fn add_message(&self, message: &Message) -> Result<()> {
        let citations = if message.role == "assistant" {
            raggy_rag::verify_citations(&message.content, &message.citations)
        } else {
            Vec::new()
        };
        let anchored = message.role == "assistant" && !citations.is_empty();
        let citations = serde_json::to_string(&citations).unwrap_or_else(|_| "[]".to_string());
        self.connection().execute(
            "INSERT INTO messages (id, session_id, role, content, anchored, citations, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                message.id,
                message.session_id,
                message.role,
                message.content,
                anchored as i64,
                citations,
                message.created_at,
            ],
        )?;
        Ok(())
    }

    /// Lista los mensajes de una sesión en orden cronológico.
    pub fn list_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, session_id, role, content, created_at, citations
             FROM messages WHERE session_id = ?1 ORDER BY created_at ASC, rowid ASC",
        )?;
        let rows = stmt.query_map([session_id], row_to_message)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // -------------------------------------------------------------- artefactos

    /// Registra un artefacto generado (HU-20).
    pub fn add_artifact(&self, artifact: &Artifact) -> Result<()> {
        self.connection().execute(
            "INSERT INTO artifacts
                 (id, session_id, name, relative_path, path, language, content, model, archived, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                artifact.id,
                artifact.session_id,
                artifact.name,
                artifact.relative_path,
                artifact.path,
                artifact.language,
                artifact.content,
                artifact.model,
                artifact.archived as i64,
                artifact.created_at,
            ],
        )?;
        Ok(())
    }

    /// Lista los artefactos de una sesión (HU-20).
    pub fn list_artifacts_by_session(&self, session_id: &str) -> Result<Vec<Artifact>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, session_id, name, relative_path, path, language, content, model,
                    archived, created_at
             FROM artifacts WHERE session_id = ?1 ORDER BY relative_path ASC, created_at ASC",
        )?;
        let rows = stmt.query_map([session_id], row_to_artifact)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Lista los artefactos de todas las sesiones de una carpeta (HU-20, HU-22).
    pub fn list_artifacts_by_folder(&self, folder_id: &str) -> Result<Vec<Artifact>> {
        let mut stmt = self.connection().prepare(
            "SELECT a.id, a.session_id, a.name, a.relative_path, a.path, a.language, a.content,
                    a.model, a.archived, a.created_at
             FROM artifacts a
             JOIN sessions s ON s.id = a.session_id
             WHERE s.folder_id = ?1
             ORDER BY a.relative_path ASC, a.created_at ASC",
        )?;
        let rows = stmt.query_map([folder_id], row_to_artifact)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Marca o desmarca un artefacto como archivado.
    pub fn set_artifact_archived(&self, id: &str, archived: bool) -> Result<()> {
        self.connection().execute(
            "UPDATE artifacts SET archived = ?2 WHERE id = ?1",
            params![id, archived as i64],
        )?;
        Ok(())
    }

    /// Elimina el registro de un artefacto (sin borrar el archivo físico).
    pub fn delete_artifact(&self, id: &str) -> Result<()> {
        self.connection()
            .execute("DELETE FROM artifacts WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Actualiza la ubicación física de un artefacto restaurado desde un
    /// respaldo.
    pub fn update_artifact_path(&self, id: &str, path: &str) -> Result<()> {
        self.connection().execute(
            "UPDATE artifacts SET path = ?2 WHERE id = ?1",
            params![id, path],
        )?;
        Ok(())
    }
}

fn row_to_artifact(row: &Row<'_>) -> rusqlite::Result<Artifact> {
    Ok(Artifact {
        id: row.get(0)?,
        session_id: row.get(1)?,
        name: row.get(2)?,
        relative_path: row.get(3)?,
        path: row.get(4)?,
        language: row.get(5)?,
        content: row.get(6)?,
        model: row.get(7)?,
        archived: row.get::<_, i64>(8)? != 0,
        created_at: row.get(9)?,
    })
}

fn row_to_folder(row: &Row<'_>) -> rusqlite::Result<Folder> {
    let status: String = row.get(3)?;
    Ok(Folder {
        id: row.get(0)?,
        path: row.get(1)?,
        name: row.get(2)?,
        status: FolderStatus::from_stored(&status).unwrap_or(FolderStatus::Pending),
        file_count: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn row_to_session(row: &Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        folder_id: row.get(1)?,
        title: row.get(2)?,
        provider: row.get(3)?,
        model: row.get(4)?,
        fallback_provider: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn row_to_message(row: &Row<'_>) -> rusqlite::Result<Message> {
    let candidates: Option<String> = row.get(5)?;
    let candidates: Vec<raggy_rag::Citation> = candidates
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    let role: String = row.get(2)?;
    let content: String = row.get(3)?;
    let citations = if role == "assistant" {
        raggy_rag::verify_citations(&content, &candidates)
    } else {
        Vec::new()
    };
    let anchored = role == "assistant" && !citations.is_empty();

    Ok(Message {
        id: row.get(0)?,
        session_id: row.get(1)?,
        role,
        content,
        anchored,
        citations,
        created_at: row.get(4)?,
    })
}

fn row_to_document(row: &Row<'_>) -> rusqlite::Result<Document> {
    Ok(Document {
        id: row.get(0)?,
        folder_id: row.get(1)?,
        relative_path: row.get(2)?,
        content_hash: row.get(3)?,
        processing_hash: row.get(4)?,
        size_bytes: row.get(5)?,
        modified_at: row.get(6)?,
        indexed_at: row.get(7)?,
        chunk_count: row.get(8)?,
    })
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Codifica un vector de `f32` como bytes little-endian.
fn encode_vector(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Decodifica un vector almacenado como bytes little-endian.
fn decode_vector(bytes: Vec<u8>) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}
