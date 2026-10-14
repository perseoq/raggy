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
}
