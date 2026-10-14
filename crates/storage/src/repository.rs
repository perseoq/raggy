//! Consultas y mutaciones sobre SQLite para las entidades del dominio.

use rusqlite::{params, OptionalExtension, Row};

use raggy_rag::Chunk;

use crate::error::Result;
use crate::models::{Document, Folder, FolderStatus};
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

    /// Actualiza el estado y el contador de archivos de una carpeta.
    pub fn set_folder_status(&self, id: &str, status: FolderStatus, file_count: i64) -> Result<()> {
        self.connection().execute(
            "UPDATE folders SET status = ?2, file_count = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, status.as_str(), file_count, now_rfc3339()],
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
            "SELECT id, folder_id, relative_path, content_hash, size_bytes, modified_at,
                    indexed_at, chunk_count
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
                "SELECT id, folder_id, relative_path, content_hash, size_bytes, modified_at,
                        indexed_at, chunk_count
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
                 (id, folder_id, relative_path, content_hash, size_bytes, modified_at, indexed_at, chunk_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(folder_id, relative_path) DO UPDATE SET
                 content_hash = excluded.content_hash,
                 size_bytes = excluded.size_bytes,
                 modified_at = excluded.modified_at,
                 indexed_at = excluded.indexed_at,
                 chunk_count = excluded.chunk_count",
            params![
                document.id,
                document.folder_id,
                document.relative_path,
                document.content_hash,
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
                     (id, folder_id, source_path, source_file, heading, start_line, end_line,
                      content_hash, content, embedding)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            for (chunk, embedding) in chunks.iter().zip(embeddings) {
                stmt.execute(params![
                    chunk.id,
                    folder_id,
                    source_path,
                    chunk.metadata.source_file,
                    chunk.metadata.heading,
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
            "SELECT id, source_path, source_file, heading, start_line, end_line,
                    content_hash, content, embedding
             FROM chunks WHERE folder_id = ?1 ORDER BY source_path ASC, start_line ASC",
        )?;
        let rows = stmt.query_map([folder_id], |row| {
            let blob: Option<Vec<u8>> = row.get(8)?;
            Ok((
                Chunk {
                    id: row.get(0)?,
                    content_hash: row.get(6)?,
                    content: row.get(7)?,
                    metadata: raggy_rag::ChunkMetadata {
                        source_path: row.get(1)?,
                        source_file: row.get(2)?,
                        heading: row.get(3)?,
                        start_line: row.get::<_, i64>(4)? as usize,
                        end_line: row.get::<_, i64>(5)? as usize,
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
            "SELECT id, source_path, source_file, heading, start_line, end_line, content_hash, content
             FROM chunks WHERE folder_id = ?1 AND source_path = ?2 ORDER BY start_line ASC",
        )?;
        let rows = stmt.query_map(params![folder_id, source_path], |row| {
            Ok(Chunk {
                id: row.get(0)?,
                content_hash: row.get(6)?,
                content: row.get(7)?,
                metadata: raggy_rag::ChunkMetadata {
                    source_path: row.get(1)?,
                    source_file: row.get(2)?,
                    heading: row.get(3)?,
                    start_line: row.get::<_, i64>(4)? as usize,
                    end_line: row.get::<_, i64>(5)? as usize,
                },
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
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

fn row_to_document(row: &Row<'_>) -> rusqlite::Result<Document> {
    Ok(Document {
        id: row.get(0)?,
        folder_id: row.get(1)?,
        relative_path: row.get(2)?,
        content_hash: row.get(3)?,
        size_bytes: row.get(4)?,
        modified_at: row.get(5)?,
        indexed_at: row.get(6)?,
        chunk_count: row.get(7)?,
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
