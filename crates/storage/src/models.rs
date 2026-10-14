//! Entidades del dominio persistidas en SQLite.

use serde::{Deserialize, Serialize};

/// Estado del proceso de indexación de una carpeta (HU-01, HU-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FolderStatus {
    /// Añadida pero aún no procesada.
    Pending,
    /// Indexación en curso.
    Indexing,
    /// Indexada correctamente.
    Indexed,
    /// La última indexación terminó con error.
    Error,
}

impl FolderStatus {
    /// Representación persistible.
    pub fn as_str(self) -> &'static str {
        match self {
            FolderStatus::Pending => "pending",
            FolderStatus::Indexing => "indexing",
            FolderStatus::Indexed => "indexed",
            FolderStatus::Error => "error",
        }
    }

    /// Interpreta el valor almacenado en la base de datos.
    pub fn from_stored(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(FolderStatus::Pending),
            "indexing" => Some(FolderStatus::Indexing),
            "indexed" => Some(FolderStatus::Indexed),
            "error" => Some(FolderStatus::Error),
            _ => None,
        }
    }
}

/// Carpeta indexada (HU-01).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    /// Identificador único.
    pub id: String,
    /// Ruta absoluta en el sistema de archivos.
    pub path: String,
    /// Nombre mostrado en la interfaz.
    pub name: String,
    /// Estado actual de indexación.
    pub status: FolderStatus,
    /// Número de archivos `.md` procesados.
    pub file_count: i64,
    /// Fecha de creación en RFC 3339.
    pub created_at: String,
    /// Fecha de última actualización en RFC 3339.
    pub updated_at: String,
}

impl Folder {
    /// Crea una carpeta en estado [`FolderStatus::Pending`].
    pub fn new(path: impl Into<String>, name: impl Into<String>) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            path: path.into(),
            name: name.into(),
            status: FolderStatus::Pending,
            file_count: 0,
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

/// Sesión de chat vinculada a una carpeta (HU-05, HU-06).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// Identificador único.
    pub id: String,
    /// Carpeta a la que pertenece la sesión.
    pub folder_id: String,
    /// Título de la sesión.
    pub title: String,
    /// Proveedor de LLM seleccionado.
    pub provider: String,
    /// Modelo seleccionado.
    pub model: String,
    /// Fecha de creación en RFC 3339.
    pub created_at: String,
    /// Fecha de última actualización en RFC 3339.
    pub updated_at: String,
}

impl Session {
    /// Crea una sesión con fecha/hora como título por defecto (HU-05).
    pub fn new(
        folder_id: impl Into<String>,
        provider: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            folder_id: folder_id.into(),
            title: now.format("%Y-%m-%d %H:%M").to_string(),
            provider: provider.into(),
            model: model.into(),
            created_at: now.to_rfc3339(),
            updated_at: now.to_rfc3339(),
        }
    }
}

/// Mensaje dentro de una sesión (HU-07, HU-12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// Identificador único.
    pub id: String,
    /// Sesión a la que pertenece.
    pub session_id: String,
    /// Rol: `system`, `user` o `assistant`.
    pub role: String,
    /// Contenido textual.
    pub content: String,
    /// Si la respuesta está anclada a fuentes del RAG (HU-11).
    pub anchored: bool,
    /// Fecha de creación en RFC 3339.
    pub created_at: String,
}

/// Artefacto de código generado y descargado (HU-20).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// Identificador único.
    pub id: String,
    /// Sesión en la que se generó.
    pub session_id: String,
    /// Nombre del archivo declarado por el modelo.
    pub name: String,
    /// Ruta donde se guardó en disco.
    pub path: String,
    /// Modelo que lo generó, si se conoce.
    pub model: Option<String>,
    /// Fecha de creación en RFC 3339.
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_status_es_biyectivo_con_su_texto() {
        for status in [
            FolderStatus::Pending,
            FolderStatus::Indexing,
            FolderStatus::Indexed,
            FolderStatus::Error,
        ] {
            assert_eq!(FolderStatus::from_stored(status.as_str()), Some(status));
        }
        assert_eq!(FolderStatus::from_stored("desconocido"), None);
    }

    #[test]
    fn las_entidades_nuevas_generan_id_y_fecha() {
        let folder = Folder::new("/tmp/docs", "docs");
        assert!(!folder.id.is_empty());
        assert!(!folder.created_at.is_empty());
        assert_eq!(folder.status, FolderStatus::Pending);

        let session = Session::new(&folder.id, "deepseek", "deepseek-chat");
        assert_eq!(session.folder_id, folder.id);
        assert!(!session.title.is_empty());
    }
}
