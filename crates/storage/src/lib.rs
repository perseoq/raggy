//! `localmind-storage` — capa de persistencia local.
//!
//! Envoltorio sobre SQLite (vía `rusqlite`) que gestiona el esquema y las
//! entidades del dominio: carpetas indexadas, sesiones por carpeta, mensajes,
//! chunks y artefactos generados.

pub mod db;
pub mod error;
pub mod migrations;
pub mod models;

pub use db::Storage;
pub use error::{Result, StorageError};
pub use migrations::LATEST_SCHEMA_VERSION;
pub use models::{Artifact, Folder, FolderStatus, Message, Session};
