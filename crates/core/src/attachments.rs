//! Archivos adjuntos a una sesión de chat (HU-34).
//!
//! Un adjunto se copia a `attachments/<sesión>/`, se extrae y trocea con el
//! mismo pipeline que las carpetas y se indexa **bajo la sesión**. Así el chat
//! busca a la vez en el índice de su carpeta y en sus adjuntos, y puede comparar
//! la información de ambos. Quitar un adjunto o borrar la sesión elimina sus
//! fragmentos, su documento y el archivo almacenado.

use std::path::{Path, PathBuf};

use raggy_storage::{Document, IndexOwner, Storage};

use crate::error::{CoreError, Result};
use crate::index::{
    chunk_file, embed_chunks, processing_fingerprint, read_file_limited, IndexOptions,
};

/// Tamaño máximo de un archivo adjunto (50 MB).
pub const MAX_ATTACHMENT_BYTES: u64 = 50 * 1024 * 1024;

/// Tamaño máximo del conjunto de adjuntos de una sesión (200 MB).
pub const MAX_SESSION_ATTACHMENT_BYTES: i64 = 200 * 1024 * 1024;

/// Resultado de adjuntar archivos a una sesión.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttachmentSummary {
    /// Nombres de los adjuntos indexados.
    pub attached: Vec<String>,
    /// Nombres de los adjuntos que sustituyeron a otro anterior (mismo nombre
    /// con contenido distinto, o el mismo contenido con otro nombre).
    pub replaced: Vec<String>,
    /// Orígenes omitidos y el motivo.
    pub skipped: Vec<AttachmentSkip>,
    /// Avisos de la extracción (por ejemplo, un PDF escaneado sin OCR que no
    /// aportó texto). Se muestran en la interfaz: si no, el adjunto parece
    /// indexado pero no aporta nada al chat.
    pub warnings: Vec<String>,
}

impl AttachmentSummary {
    /// ¿No se adjuntó ningún archivo?
    pub fn is_empty(&self) -> bool {
        self.attached.is_empty()
    }
}

/// Archivo omitido al adjuntar, con el motivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentSkip {
    /// Ruta original del archivo.
    pub path: PathBuf,
    /// Motivo por el que no se adjuntó.
    pub reason: String,
}

/// Adjunta archivos a una sesión: los copia, los indexa y devuelve el resumen.
///
/// `directory` es la carpeta física de adjuntos de la sesión
/// (`attachments/<sesión>`). Los archivos que superan los límites, no son
/// indexables o no se pueden leer se omiten con su motivo, sin abortar el resto.
pub fn attach_files(
    storage: &mut Storage,
    session_id: &str,
    directory: &Path,
    paths: &[PathBuf],
    options: &IndexOptions,
) -> Result<AttachmentSummary> {
    std::fs::create_dir_all(directory).map_err(|source| CoreError::Io {
        path: directory.to_path_buf(),
        source,
    })?;

    let processing_hash = processing_fingerprint(options);
    let mut existing = storage.list_session_documents(session_id)?;
    let mut used_bytes: i64 = existing.iter().map(|document| document.size_bytes).sum();
    let mut summary = AttachmentSummary::default();

    for path in paths {
        if !path.is_file() {
            push_skip(&mut summary, path, "no es un archivo");
            continue;
        }
        if !crate::index::is_indexable(path) {
            push_skip(&mut summary, path, "no es un formato indexable");
            continue;
        }

        let size_bytes = std::fs::metadata(path).map_or(0, |metadata| metadata.len() as i64);
        if size_bytes == 0 {
            push_skip(&mut summary, path, "está vacío");
            continue;
        }
        if size_bytes as u64 > MAX_ATTACHMENT_BYTES {
            push_skip(
                &mut summary,
                path,
                &format!(
                    "supera el límite de {} MB por archivo",
                    MAX_ATTACHMENT_BYTES / (1024 * 1024)
                ),
            );
            continue;
        }
        if used_bytes + size_bytes > MAX_SESSION_ATTACHMENT_BYTES {
            push_skip(
                &mut summary,
                path,
                "supera el límite total de 200 MB por sesión; quita algún adjunto",
            );
            continue;
        }

        let bytes = match read_file_limited(path, MAX_ATTACHMENT_BYTES) {
            Ok(bytes) => bytes,
            Err(error) => {
                push_skip(&mut summary, path, &format!("no se pudo leer: {error}"));
                continue;
            }
        };
        let content_hash = blake3::hash(&bytes).to_hex().to_string();

        let name = sanitize_name(&path.file_name().unwrap_or_default().to_string_lossy());
        // Si ya hay un documento con el mismo contenido, se reemplaza para no
        // duplicar el mismo archivo con nombres distintos.
        if let Some(duplicate) = existing
            .iter()
            .find(|document| document.content_hash == content_hash)
            .map(|document| document.relative_path.clone())
        {
            if duplicate != name {
                remove_stored(storage, session_id, directory, &duplicate)?;
                if let Some(position) = existing
                    .iter()
                    .position(|document| document.relative_path == duplicate)
                {
                    used_bytes -= existing[position].size_bytes;
                    existing.remove(position);
                }
                summary.replaced.push(duplicate);
            }
        }

        // Nombre final: si ya hay otro archivo con ese nombre y contenido
        // distinto, se conserva el anterior y el nuevo se guarda con un sufijo.
        let relative = match existing
            .iter()
            .find(|document| document.relative_path == name)
        {
            Some(document) if document.content_hash == content_hash => name,
            Some(_) => free_name(directory, &name),
            None if directory.join(&name).is_file() => free_name(directory, &name),
            None => name,
        };

        let destination = directory.join(&relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|source| CoreError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        if let Err(error) = std::fs::copy(path, &destination) {
            push_skip(&mut summary, path, &format!("no se pudo copiar: {error}"));
            continue;
        }

        let (chunks, warnings) = match chunk_file(path, &relative, &bytes, options) {
            Ok(result) => result,
            Err(error) => {
                let _ = std::fs::remove_file(&destination);
                push_skip(&mut summary, path, &format!("no se pudo extraer: {error}"));
                continue;
            }
        };
        for warning in &warnings {
            tracing::warn!(path = %path.display(), "{warning}");
            summary.warnings.push(format!("`{relative}`: {warning}"));
        }
        if chunks.is_empty() {
            // Sin fragmentos el adjunto no aporta nada al chat: hay que decirlo.
            let aviso = format!(
                "`{relative}` no aportó texto: puede estar escaneado o ser una imagen; \
                 activa OCR en Configuración (con Tesseract instalado) y vuelve a adjuntarlo."
            );
            tracing::warn!(path = %path.display(), "{aviso}");
            summary.warnings.push(aviso);
        }

        let embeddings = embed_chunks(&chunks, options.embedding_dimensions);
        let document = Document::for_session(
            session_id,
            relative.clone(),
            content_hash,
            processing_hash.clone(),
            size_bytes,
            modified_millis(path),
            chunks.len() as i64,
        );
        storage.replace_file_index(
            IndexOwner::Session(session_id),
            &relative,
            &chunks,
            &embeddings,
            &document,
        )?;

        used_bytes += size_bytes;
        existing.retain(|document| document.relative_path != relative);
        existing.push(document);
        summary.attached.push(relative);
    }

    Ok(summary)
}

/// Quita un adjunto: sus fragmentos, su documento y el archivo almacenado.
pub fn remove_attachment(
    storage: &mut Storage,
    session_id: &str,
    directory: &Path,
    relative_path: &str,
) -> Result<()> {
    if crate::artifacts::sanitize_relative(relative_path).as_deref() != Some(relative_path) {
        return Err(CoreError::InvalidAttachmentName {
            name: relative_path.to_string(),
        });
    }
    remove_stored(storage, session_id, directory, relative_path)
}

/// Quita todos los adjuntos de una sesión (índices y archivos almacenados).
pub fn clear_attachments(storage: &mut Storage, session_id: &str, directory: &Path) -> Result<()> {
    storage.delete_session_index(session_id)?;
    if directory.is_dir() {
        std::fs::remove_dir_all(directory).map_err(|source| CoreError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
    }
    Ok(())
}

/// Reindexa los adjuntos cuyo índice se hizo con otro pipeline.
///
/// Es el equivalente de la reindexación automática de carpetas para los archivos
/// adjuntos a los chats: si cambió la versión del pipeline, las opciones de
/// ingesta o las herramientas externas, el adjunto almacenado se vuelve a
/// extraer y trocear. Devuelve cuántos adjuntos se reindexaron.
pub fn reindex_stale_attachments(
    storage: &mut Storage,
    attachments_root: &Path,
    options: &IndexOptions,
) -> Result<usize> {
    let current = processing_fingerprint(options);
    let mut reindexed = 0usize;

    let folders = storage.list_folders()?;
    for folder in folders {
        let sessions = storage.list_sessions(&folder.id)?;
        for session in sessions {
            let directory = attachments_root.join(&session.id);
            let documents = storage.list_session_documents(&session.id)?;
            for document in documents {
                if document.processing_hash == current {
                    continue;
                }
                let Some(path) = attachment_path(&directory, &document.relative_path) else {
                    continue;
                };
                if !path.is_file() {
                    // Sin archivo almacenado no hay nada que reindexar.
                    continue;
                }
                let bytes = match read_file_limited(&path, MAX_ATTACHMENT_BYTES) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        tracing::warn!(
                            path = %path.display(),
                            "no se pudo releer el adjunto: {error}"
                        );
                        continue;
                    }
                };

                let (chunks, warnings) =
                    match chunk_file(&path, &document.relative_path, &bytes, options) {
                        Ok(result) => result,
                        Err(error) => {
                            tracing::warn!(
                                path = %path.display(),
                                "no se pudo reindexar el adjunto: {error}"
                            );
                            continue;
                        }
                    };
                for warning in &warnings {
                    tracing::warn!(path = %path.display(), "{warning}");
                }

                let embeddings = embed_chunks(&chunks, options.embedding_dimensions);
                let updated = Document::for_session(
                    &session.id,
                    document.relative_path.clone(),
                    document.content_hash.clone(),
                    current.clone(),
                    document.size_bytes,
                    modified_millis(&path),
                    chunks.len() as i64,
                );
                storage.replace_file_index(
                    IndexOwner::Session(&session.id),
                    &document.relative_path,
                    &chunks,
                    &embeddings,
                    &updated,
                )?;
                reindexed += 1;
            }
        }
    }

    Ok(reindexed)
}

/// Ruta física de un adjunto dentro de la carpeta de su sesión.
pub fn attachment_path(directory: &Path, relative_path: &str) -> Option<PathBuf> {
    crate::artifacts::sanitize_relative(relative_path).map(|relative| directory.join(relative))
}

fn remove_stored(
    storage: &mut Storage,
    session_id: &str,
    directory: &Path,
    relative_path: &str,
) -> Result<()> {
    storage.delete_chunks_for_path(IndexOwner::Session(session_id), relative_path)?;
    storage.delete_document(IndexOwner::Session(session_id), relative_path)?;
    if let Some(path) = attachment_path(directory, relative_path) {
        if path.is_file() {
            let _ = std::fs::remove_file(&path);
        }
    }
    Ok(())
}

fn push_skip(summary: &mut AttachmentSummary, path: &Path, reason: &str) {
    let reason = reason.to_string();
    tracing::warn!(path = %path.display(), "adjunto omitido: {reason}");
    summary.skipped.push(AttachmentSkip {
        path: path.to_path_buf(),
        reason,
    });
}

/// Reduce el nombre a un único componente seguro para el sistema de archivos.
fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => character,
        })
        .collect();
    // El nombre nunca debe poder salir de la carpeta de la sesión.
    let cleaned = cleaned.replace("..", "_");
    let trimmed = cleaned.trim().trim_start_matches('.').trim().to_string();
    if trimmed.is_empty() {
        "adjunto".to_string()
    } else {
        trimmed
    }
}

/// Nombre libre dentro de la carpeta de la sesión, conservando la extensión.
fn free_name(directory: &Path, name: &str) -> String {
    crate::artifacts::free_path(&directory.join(name))
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string())
}

fn modified_millis(path: &Path) -> i64 {
    std::fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use raggy_storage::{Folder, Session};

    fn temp_dir(tag: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("raggy-adjuntos-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("crea directorio temporal");
        path
    }

    /// Crea una carpeta y una sesión en un almacén en memoria.
    ///
    /// Cada prueba usa su propia etiqueta: compartir el directorio temporal haría
    /// que una prueba borrara los archivos de otra al correr en paralelo.
    fn sesion_de_prueba(tag: &str) -> (Storage, String, PathBuf) {
        let storage = Storage::open_in_memory().expect("abre SQLite");
        let folder = Folder::new("/tmp/adjuntos-prueba", "adjuntos");
        storage.upsert_folder(&folder).expect("guarda carpeta");
        let session = Session::new(&folder.id, "deepseek", "deepseek-chat");
        storage.upsert_session(&session).expect("guarda sesión");
        let directory = temp_dir(tag).join("adjuntos").join(&session.id);
        (storage, session.id, directory)
    }

    #[test]
    fn adjunta_indexa_y_quita_un_archivo() {
        let (mut storage, session_id, directory) = sesion_de_prueba("adjunta");
        let origin = temp_dir("adjunta-origen");
        let informe = origin.join("informe.md");
        std::fs::write(
            &informe,
            "# Informe\n\nEl presupuesto del hotel es de 120 euros por noche.\n",
        )
        .expect("escribe el adjunto");

        let summary = attach_files(
            &mut storage,
            &session_id,
            &directory,
            std::slice::from_ref(&informe),
            &IndexOptions::default(),
        )
        .expect("adjunta");

        assert_eq!(summary.attached, vec!["informe.md".to_string()]);
        assert!(summary.skipped.is_empty());
        assert!(
            directory.join("informe.md").is_file(),
            "se guarda una copia"
        );

        let documentos = storage.list_session_documents(&session_id).expect("lista");
        assert_eq!(documentos.len(), 1);
        assert_eq!(documentos[0].relative_path, "informe.md");
        assert!(documentos[0].chunk_count >= 1);
        assert!(!storage
            .list_embedded_chunks_for_session(&session_id)
            .expect("fragmentos")
            .is_empty());

        remove_attachment(&mut storage, &session_id, &directory, "informe.md").expect("quita");
        assert!(storage
            .list_session_documents(&session_id)
            .expect("lista")
            .is_empty());
        assert!(!directory.join("informe.md").exists(), "se borra la copia");

        let _ = std::fs::remove_dir_all(&origin);
    }

    #[test]
    fn adjuntar_el_mismo_contenido_dos_veces_no_duplica() {
        let (mut storage, session_id, directory) = sesion_de_prueba("duplicado");
        let origin = temp_dir("duplicado-origen");
        let primero = origin.join("informe.md");
        std::fs::write(&primero, "# Informe\n\nmismo contenido\n").expect("escribe");
        let copia = origin.join("informe-copia.md");
        std::fs::write(&copia, "# Informe\n\nmismo contenido\n").expect("escribe la copia");

        attach_files(
            &mut storage,
            &session_id,
            &directory,
            std::slice::from_ref(&primero),
            &IndexOptions::default(),
        )
        .expect("adjunta el primero");
        let summary = attach_files(
            &mut storage,
            &session_id,
            &directory,
            std::slice::from_ref(&copia),
            &IndexOptions::default(),
        )
        .expect("adjunta la copia");

        assert_eq!(summary.attached, vec!["informe-copia.md".to_string()]);
        assert_eq!(
            storage
                .list_session_documents(&session_id)
                .expect("lista")
                .len(),
            1,
            "el mismo contenido no debe quedar duplicado"
        );
        assert!(
            !directory.join("informe.md").exists(),
            "se retira la copia anterior"
        );

        let _ = std::fs::remove_dir_all(&origin);
    }

    #[test]
    fn omite_archivos_no_indexables_y_demasiado_grandes() {
        let (mut storage, session_id, directory) = sesion_de_prueba("omitidos");
        let origin = temp_dir("omitidos-origen");
        let imagen = origin.join("foto.png");
        std::fs::write(&imagen, b"no es indexable").expect("escribe la imagen");
        let vacio = origin.join("vacio.md");
        std::fs::write(&vacio, "").expect("escribe el vacío");

        let summary = attach_files(
            &mut storage,
            &session_id,
            &directory,
            &[imagen, vacio],
            &IndexOptions::default(),
        )
        .expect("adjunta");

        assert!(summary.is_empty());
        assert_eq!(summary.skipped.len(), 2);
        assert!(summary
            .skipped
            .iter()
            .any(|skip| skip.reason.contains("indexable")));
        assert!(summary
            .skipped
            .iter()
            .any(|skip| skip.reason.contains("vacío")));

        let _ = std::fs::remove_dir_all(&origin);
    }

    #[test]
    fn usa_un_nombre_libre_si_ya_hay_otro_archivo_con_ese_nombre() {
        let (mut storage, session_id, directory) = sesion_de_prueba("colision");
        let origin = temp_dir("colision-origen");
        let primero = origin.join("a").join("informe.md");
        std::fs::create_dir_all(primero.parent().expect("padre")).expect("crea carpeta");
        std::fs::write(&primero, "# Primero\n\ncontenido uno\n").expect("escribe el primero");
        let segundo = origin.join("b").join("informe.md");
        std::fs::create_dir_all(segundo.parent().expect("padre")).expect("crea carpeta");
        std::fs::write(&segundo, "# Segundo\n\ncontenido dos\n").expect("escribe el segundo");

        attach_files(
            &mut storage,
            &session_id,
            &directory,
            std::slice::from_ref(&primero),
            &IndexOptions::default(),
        )
        .expect("adjunta el primero");
        let summary = attach_files(
            &mut storage,
            &session_id,
            &directory,
            std::slice::from_ref(&segundo),
            &IndexOptions::default(),
        )
        .expect("adjunta el segundo");

        assert_eq!(summary.attached, vec!["informe_1.md".to_string()]);
        assert!(directory.join("informe.md").is_file());
        assert!(directory.join("informe_1.md").is_file());
        assert_eq!(
            storage
                .list_session_documents(&session_id)
                .expect("lista")
                .len(),
            2
        );

        let _ = std::fs::remove_dir_all(&origin);
    }

    #[test]
    fn limpiar_los_adjuntos_borra_indices_y_archivos() {
        let (mut storage, session_id, directory) = sesion_de_prueba("limpiar");
        let origin = temp_dir("limpiar-origen");
        let informe = origin.join("informe.md");
        std::fs::write(&informe, "# Informe\n\ncontenido\n").expect("escribe");

        attach_files(
            &mut storage,
            &session_id,
            &directory,
            std::slice::from_ref(&informe),
            &IndexOptions::default(),
        )
        .expect("adjunta");
        clear_attachments(&mut storage, &session_id, &directory).expect("limpia");

        assert!(storage
            .list_session_documents(&session_id)
            .expect("lista")
            .is_empty());
        assert!(!directory.exists(), "se borra la carpeta de adjuntos");

        let _ = std::fs::remove_dir_all(&origin);
    }

    #[test]
    fn sanea_los_nombres_de_adjunto() {
        let name = sanitize_name("../../etc/passwd");
        assert!(!name.contains('/'), "sin separadores: {name}");
        assert!(
            !name.contains(".."),
            "sin poder salir de la carpeta: {name}"
        );
        assert_eq!(sanitize_name("carpeta/informe.md"), "carpeta_informe.md");
        assert_eq!(sanitize_name("   "), "adjunto");
        assert_eq!(sanitize_name(".oculto.md"), "oculto.md");
    }

    #[test]
    fn avisa_cuando_un_adjunto_no_aporta_texto() {
        let (mut storage, session_id, directory) = sesion_de_prueba("sin-texto");
        let origin = temp_dir("sin-texto-origen");
        let vacio = origin.join("escaneado.txt");
        std::fs::write(&vacio, "   \n\n   \n").expect("escribe el adjunto");

        let summary = attach_files(
            &mut storage,
            &session_id,
            &directory,
            std::slice::from_ref(&vacio),
            &IndexOptions::default(),
        )
        .expect("adjunta");

        assert_eq!(summary.attached, vec!["escaneado.txt".to_string()]);
        assert!(
            summary
                .warnings
                .iter()
                .any(|warning| warning.contains("no aportó texto")),
            "un adjunto sin texto debe avisar: {:?}",
            summary.warnings
        );
        let documentos = storage.list_session_documents(&session_id).expect("lista");
        assert_eq!(documentos[0].chunk_count, 0);

        let _ = std::fs::remove_dir_all(&origin);
    }

    #[test]
    fn reindexa_solo_los_adjuntos_con_el_pipeline_anticuado() {
        let (mut storage, session_id, directory) = sesion_de_prueba("reindexa");
        let origin = temp_dir("reindexa-origen");
        let informe = origin.join("informe.md");
        let texto = format!(
            "# Informe\n\n{}",
            "Parrafo de relleno con texto suficiente para trocear. ".repeat(40)
        );
        std::fs::write(&informe, texto).expect("escribe el adjunto");

        let original = IndexOptions::default();
        attach_files(
            &mut storage,
            &session_id,
            &directory,
            std::slice::from_ref(&informe),
            &original,
        )
        .expect("adjunta");

        let root = directory.parent().expect("raíz de adjuntos").to_path_buf();
        assert_eq!(
            reindex_stale_attachments(&mut storage, &root, &original).expect("reindexa"),
            0,
            "con el mismo pipeline no hay nada que rehacer"
        );

        let mut nuevo = original.clone();
        nuevo.chunking.max_chars /= 2;
        assert_eq!(
            reindex_stale_attachments(&mut storage, &root, &nuevo).expect("reindexa"),
            1,
            "el adjunto desfasado se reindexa"
        );

        let documentos = storage.list_session_documents(&session_id).expect("lista");
        assert_eq!(documentos.len(), 1, "no se duplica el adjunto");
        assert_eq!(
            documentos[0].processing_hash,
            processing_fingerprint(&nuevo),
            "queda marcado con la huella nueva"
        );

        let _ = std::fs::remove_dir_all(&origin);
    }
}
