//! Esquema de la base de datos y su aplicación incremental.
//!
//! Las migraciones usan `PRAGMA user_version` como contador de versión. Para
//! añadir una migración basta con agregar un elemento a [`MIGRATIONS`] al
//! final; nunca se deben modificar las migraciones ya publicadas.

use rusqlite::Connection;

use crate::error::Result;

/// Una migración del esquema, identificada por una versión monotónica.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    /// Número de versión que deja aplicada.
    pub version: i64,
    /// Descripción corta para trazabilidad.
    pub description: &'static str,
    /// Sentencias SQL a ejecutar de forma atómica.
    pub sql: &'static str,
}

/// Migraciones conocidas, en orden creciente de versión.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "esquema inicial: carpetas, chunks, sesiones, mensajes y artefactos",
        sql: include_str!("../migrations/0001_initial.sql"),
    },
    Migration {
        version: 2,
        description: "seguimiento por archivo para re-indexación incremental",
        sql: include_str!("../migrations/0002_documents.sql"),
    },
    Migration {
        version: 3,
        description: "vector de embedding por fragmento",
        sql: include_str!("../migrations/0003_chunk_embeddings.sql"),
    },
    Migration {
        version: 4,
        description: "citas por mensaje del asistente",
        sql: include_str!("../migrations/0004_message_citations.sql"),
    },
];

/// Versión de esquema que espera este binario.
pub const LATEST_SCHEMA_VERSION: i64 = 4;

/// Aplica todas las migraciones pendientes dentro de una transacción.
pub fn apply(conn: &mut Connection) -> Result<i64> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if current > LATEST_SCHEMA_VERSION {
        return Err(crate::error::StorageError::SchemaTooNew {
            found: current,
            supported: LATEST_SCHEMA_VERSION,
        });
    }

    let mut applied = current;
    for migration in MIGRATIONS.iter().filter(|m| m.version > current) {
        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql)?;
        tx.pragma_update(None, "user_version", migration.version)?;
        tx.commit()?;

        tracing::debug!(
            version = migration.version,
            description = migration.description,
            "migración aplicada"
        );
        applied = migration.version;
    }

    Ok(applied)
}
