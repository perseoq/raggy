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
    /// Proveedor de respaldo ofrecido si el principal falla (HU-25).
    pub fallback_provider: Option<String>,
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
            fallback_provider: None,
            created_at: now.to_rfc3339(),
            updated_at: now.to_rfc3339(),
        }
    }
}

/// Mensaje dentro de una sesión (HU-07, HU-12).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// Identificador único.
    pub id: String,
    /// Sesión a la que pertenece.
    pub session_id: String,
    /// Rol: `system`, `user` o `assistant`.
    pub role: String,
    /// Contenido textual.
    pub content: String,
    /// Si la respuesta contiene al menos una cita validada contra el contexto (HU-11).
    pub anchored: bool,
    /// Fuentes citadas en la generación (HU-11).
    pub citations: Vec<raggy_rag::Citation>,
    /// Fecha de creación en RFC 3339.
    pub created_at: String,
}

impl Message {
    /// Crea un mensaje nuevo. `anchored` se conserva por compatibilidad, pero
    /// el valor se recalcula a partir de las citas verificadas.
    pub fn new(
        session_id: impl Into<String>,
        role: impl Into<String>,
        content: impl Into<String>,
        _anchored: bool,
        citations: Vec<raggy_rag::Citation>,
    ) -> Self {
        let role = role.into();
        let content = content.into();
        let citations = if role == "assistant" {
            raggy_rag::verify_citations(&content, &citations)
        } else {
            Vec::new()
        };
        // No se confía en el booleano del llamador: el anclaje se deriva de las
        // citas que realmente aparecen y coinciden con las fuentes recuperadas.
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            anchored: role == "assistant" && !citations.is_empty(),
            role,
            content,
            citations,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Propietario de los fragmentos y documentos: la carpeta indexada de un chat o
/// la propia sesión (archivos adjuntos al chat).
///
/// Un registro pertenece exactamente a uno de los dos, lo que mantiene aisladas
/// las sesiones entre sí y respecto al índice de la carpeta (HU-06).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndexOwner<'a> {
    /// Carpeta indexada del chat.
    Folder(&'a str),
    /// Sesión de chat que contiene archivos adjuntos.
    Session(&'a str),
}

impl<'a> IndexOwner<'a> {
    /// Columna de la base de datos que identifica al propietario.
    pub fn column(self) -> &'static str {
        match self {
            IndexOwner::Folder(_) => "folder_id",
            IndexOwner::Session(_) => "session_id",
        }
    }

    /// Identificador del propietario.
    pub fn id(self) -> &'a str {
        match self {
            IndexOwner::Folder(id) | IndexOwner::Session(id) => id,
        }
    }

    /// Carpeta a la que pertenece, si el propietario es una carpeta.
    pub fn folder_id(self) -> Option<&'a str> {
        match self {
            IndexOwner::Folder(id) => Some(id),
            IndexOwner::Session(_) => None,
        }
    }
}

/// Archivo indexable rastreado dentro de una carpeta o adjunto a una sesión
/// (HU-02, HU-03).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    /// Identificador único.
    pub id: String,
    /// Carpeta a la que pertenece, si procede del índice de una carpeta.
    #[serde(default)]
    pub folder_id: Option<String>,
    /// Sesión a la que pertenece, si es un archivo adjunto al chat.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Ruta relativa a la raíz de la carpeta (o nombre del adjunto).
    pub relative_path: String,
    /// Hash del contenido completo del archivo.
    pub content_hash: String,
    /// Huella de la versión del extractor y sus opciones de procesamiento.
    pub processing_hash: String,
    /// Tamaño en bytes.
    pub size_bytes: i64,
    /// Fecha de modificación en milisegundos desde la época UNIX.
    pub modified_at: i64,
    /// Fecha de indexación en RFC 3339.
    pub indexed_at: String,
    /// Número de fragmentos generados.
    pub chunk_count: i64,
}

impl Document {
    /// Crea el registro de un documento indexado dentro de una carpeta.
    pub fn new(
        folder_id: impl Into<String>,
        relative_path: impl Into<String>,
        content_hash: impl Into<String>,
        processing_hash: impl Into<String>,
        size_bytes: i64,
        modified_at: i64,
        chunk_count: i64,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            folder_id: Some(folder_id.into()),
            session_id: None,
            relative_path: relative_path.into(),
            content_hash: content_hash.into(),
            processing_hash: processing_hash.into(),
            size_bytes,
            modified_at,
            indexed_at: chrono::Utc::now().to_rfc3339(),
            chunk_count,
        }
    }

    /// Crea el registro de un archivo adjunto a una sesión de chat.
    pub fn for_session(
        session_id: impl Into<String>,
        relative_path: impl Into<String>,
        content_hash: impl Into<String>,
        processing_hash: impl Into<String>,
        size_bytes: i64,
        modified_at: i64,
        chunk_count: i64,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            folder_id: None,
            session_id: Some(session_id.into()),
            relative_path: relative_path.into(),
            content_hash: content_hash.into(),
            processing_hash: processing_hash.into(),
            size_bytes,
            modified_at,
            indexed_at: chrono::Utc::now().to_rfc3339(),
            chunk_count,
        }
    }

    /// Propietario del documento, si tiene uno.
    pub fn owner(&self) -> Option<IndexOwner<'_>> {
        match (self.folder_id.as_deref(), self.session_id.as_deref()) {
            (Some(folder_id), None) => Some(IndexOwner::Folder(folder_id)),
            (None, Some(session_id)) => Some(IndexOwner::Session(session_id)),
            _ => None,
        }
    }
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
    /// Ruta relativa dentro del proyecto.
    pub relative_path: String,
    /// Ruta absoluta donde se guardó.
    pub path: String,
    /// Lenguaje del bloque de código, si se conoce.
    pub language: Option<String>,
    /// Contenido guardado, para dar continuidad (HU-21).
    pub content: String,
    /// Modelo que lo generó, si se conoce.
    pub model: Option<String>,
    /// Si el registro está archivado (no se borra el archivo físico).
    pub archived: bool,
    /// Fecha de creación en RFC 3339.
    pub created_at: String,
}

impl Artifact {
    /// Crea el registro de un artefacto recién guardado.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session_id: impl Into<String>,
        name: impl Into<String>,
        relative_path: impl Into<String>,
        path: impl Into<String>,
        language: Option<String>,
        content: impl Into<String>,
        model: Option<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            name: name.into(),
            relative_path: relative_path.into(),
            path: path.into(),
            language,
            content: content.into(),
            model,
            archived: false,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
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

    #[test]
    fn un_mensaje_solo_queda_anclado_con_una_cita_verificada() {
        let citation = raggy_rag::Citation {
            source_file: "guia.md".to_string(),
            source_path: "docs/guia.md".to_string(),
            heading: None,
            location: Some("página 2".to_string()),
            start_line: 4,
            end_line: 8,
            score: 0.9,
        };
        let unrelated = Message::new(
            "session",
            "assistant",
            "La respuesta no cita la fuente.",
            true,
            vec![citation.clone()],
        );
        assert!(!unrelated.anchored);
        assert!(unrelated.citations.is_empty());

        let cited = Message::new(
            "session",
            "assistant",
            "La respuesta cita [docs/guia.md (página 2)].",
            false,
            vec![citation.clone()],
        );
        assert!(cited.anchored);
        assert_eq!(cited.citations, vec![citation]);

        let user = Message::new(
            "session",
            "user",
            "[docs/guia.md (página 2)]",
            true,
            vec![cited.citations[0].clone()],
        );
        assert!(!user.anchored);
        assert!(user.citations.is_empty());
    }

    #[test]
    fn un_documento_pertenece_a_una_carpeta_o_a_una_sesion() {
        let folder_document = Document::new("f1", "a.md", "h", "p", 4, 0, 1);
        assert_eq!(folder_document.owner(), Some(IndexOwner::Folder("f1")));
        assert_eq!(folder_document.session_id, None);

        let attachment = Document::for_session("s1", "informe.pdf", "h", "p", 9, 0, 2);
        assert_eq!(attachment.owner(), Some(IndexOwner::Session("s1")));
        assert_eq!(attachment.folder_id, None);

        let mut corrupt = folder_document.clone();
        corrupt.session_id = Some("s1".to_string());
        assert_eq!(corrupt.owner(), None, "no puede pertenecer a los dos");
    }

    #[test]
    fn el_propietario_indica_su_columna_y_su_id() {
        assert_eq!(IndexOwner::Folder("f1").column(), "folder_id");
        assert_eq!(IndexOwner::Folder("f1").id(), "f1");
        assert_eq!(IndexOwner::Folder("f1").folder_id(), Some("f1"));

        assert_eq!(IndexOwner::Session("s1").column(), "session_id");
        assert_eq!(IndexOwner::Session("s1").id(), "s1");
        assert_eq!(IndexOwner::Session("s1").folder_id(), None);
    }
}
