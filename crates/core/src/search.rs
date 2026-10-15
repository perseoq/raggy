//! Búsqueda semántica sobre el índice de una carpeta (HU-10).
//!
//! Construye un recuperador híbrido (embeddings locales + BM25) con los
//! fragmentos persistidos y devuelve los más relevantes por encima del umbral.
//! La interfaz reutiliza `SearchCache` entre consultas y lo renueva cuando cambia
//! la revisión de la carpeta.

use std::collections::HashMap;

use raggy_rag::{
    ChunkOrigin, HashingEmbedding, HybridRetriever, RetrievedChunk, Retriever, RetrieverOptions,
};
use raggy_storage::Storage;

use crate::error::Result;

/// Número máximo de recuperadores en caché antes de vaciarla por completo.
const MAX_CACHED_RETRIEVERS: usize = 8;

struct CachedRetriever {
    revision: String,
    retriever: HybridRetriever,
}

/// Recuperadores reutilizables por carpeta, sesión y dimensionalidad (HU-10).
#[derive(Default)]
pub struct SearchCache {
    entries: HashMap<(String, Option<String>, usize), CachedRetriever>,
}

impl SearchCache {
    /// Busca con un índice ya construido o lo crea si cambió la carpeta.
    pub fn search_folder(
        &mut self,
        storage: &Storage,
        folder_id: &str,
        query: &str,
        options: &RetrieverOptions,
        embedding_dimensions: usize,
    ) -> Result<Vec<RetrievedChunk>> {
        self.search(
            storage,
            folder_id,
            None,
            query,
            options,
            embedding_dimensions,
        )
    }

    /// Busca en el índice de la carpeta **y** en los adjuntos de la sesión.
    ///
    /// Los fragmentos de cada origen se etiquetan para poder comparar la
    /// información de la carpeta con la de los archivos adjuntos al chat.
    pub fn search_chat(
        &mut self,
        storage: &Storage,
        folder_id: &str,
        session_id: &str,
        query: &str,
        options: &RetrieverOptions,
        embedding_dimensions: usize,
    ) -> Result<Vec<RetrievedChunk>> {
        self.search(
            storage,
            folder_id,
            Some(session_id),
            query,
            options,
            embedding_dimensions,
        )
    }

    fn search(
        &mut self,
        storage: &Storage,
        folder_id: &str,
        session_id: Option<&str>,
        query: &str,
        options: &RetrieverOptions,
        embedding_dimensions: usize,
    ) -> Result<Vec<RetrievedChunk>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }

        let Some(folder) = storage.get_folder(folder_id)? else {
            self.invalidate_folder(folder_id);
            return Ok(Vec::new());
        };

        // La revisión combina el estado de la carpeta con el de los adjuntos, de
        // modo que adjuntar o quitar un archivo reconstruya el recuperador.
        let revision = match session_id {
            Some(session_id) => format!(
                "{}|{}",
                folder.updated_at,
                storage.session_index_revision(session_id)?
            ),
            None => folder.updated_at.clone(),
        };

        let key = (
            folder_id.to_string(),
            session_id.map(str::to_string),
            embedding_dimensions,
        );
        let needs_rebuild = self
            .entries
            .get(&key)
            .is_none_or(|cached| cached.revision != revision);
        if needs_rebuild {
            let retriever = match session_id {
                Some(session_id) => {
                    build_chat_retriever(storage, folder_id, session_id, embedding_dimensions)?
                }
                None => build_retriever(storage, folder_id, embedding_dimensions)?,
            };
            if self.entries.len() >= MAX_CACHED_RETRIEVERS {
                self.entries.clear();
            }
            self.entries.insert(
                key.clone(),
                CachedRetriever {
                    revision,
                    retriever,
                },
            );
        }

        Ok(self
            .entries
            .get(&key)
            .expect("se construyó el recuperador que faltaba")
            .retriever
            .retrieve(query, options))
    }

    /// Descarta todas las dimensionalidades cacheadas para una carpeta borrada.
    pub fn invalidate_folder(&mut self, folder_id: &str) {
        self.entries.retain(|(id, _, _), _| id != folder_id);
    }

    /// Descarta las entradas cacheadas de una sesión (p. ej. al quitar adjuntos).
    pub fn invalidate_session(&mut self, session_id: &str) {
        self.entries
            .retain(|(_, session, _), _| session.as_deref() != Some(session_id));
    }

    /// Descarta todos los recuperadores.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Cobertura de embeddings de una carpeta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbeddingCoverage {
    /// Fragmentos totales.
    pub chunks: usize,
    /// Fragmentos con embedding válido para la dimensionalidad actual.
    pub embedded: usize,
}

impl EmbeddingCoverage {
    /// ¿Conviene reindexar para regenerar los embeddings?
    pub fn needs_reindex(&self) -> bool {
        self.chunks > 0 && self.embedded < self.chunks
    }
}

/// Recupera los fragmentos más relevantes para una consulta aislada.
/// En flujos interactivos con consultas repetidas, conserva un [`SearchCache`]
/// para no reconstruir el recuperador desde SQLite en cada petición.
pub fn search_folder(
    storage: &Storage,
    folder_id: &str,
    query: &str,
    options: &RetrieverOptions,
    embedding_dimensions: usize,
) -> Result<Vec<RetrievedChunk>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let retriever = build_retriever(storage, folder_id, embedding_dimensions)?;
    let results = retriever.retrieve(query, options);
    // Diagnóstico: permite ver por qué una respuesta se queda sin contexto.
    tracing::debug!(
        folder_id,
        candidatos = results.len(),
        mejor = results.first().map(|result| result.score).unwrap_or(0.0),
        corte_relativo = options.relative_floor,
        "recuperación de contexto"
    );
    Ok(results)
}

/// Construye el recuperador híbrido a partir de los fragmentos almacenados.
pub fn build_retriever(
    storage: &Storage,
    folder_id: &str,
    embedding_dimensions: usize,
) -> Result<HybridRetriever> {
    let mut retriever = HybridRetriever::new(Box::new(HashingEmbedding::new(embedding_dimensions)));
    for (chunk, embedding) in storage.list_embedded_chunks(folder_id)? {
        retriever.add(chunk, embedding);
    }
    Ok(retriever)
}

/// Construye el recuperador de un chat: fragmentos de la carpeta del chat y de
/// los archivos adjuntos a la sesión, etiquetados por su origen.
///
/// Los adjuntos de otras sesiones nunca entran aquí, así que la información de
/// cada chat permanece aislada (HU-06).
pub fn build_chat_retriever(
    storage: &Storage,
    folder_id: &str,
    session_id: &str,
    embedding_dimensions: usize,
) -> Result<HybridRetriever> {
    let mut retriever = HybridRetriever::new(Box::new(HashingEmbedding::new(embedding_dimensions)));
    for (chunk, embedding) in storage.list_embedded_chunks(folder_id)? {
        retriever.add_with_origin(chunk, embedding, ChunkOrigin::Folder);
    }
    for (chunk, embedding) in storage.list_embedded_chunks_for_session(session_id)? {
        let name = chunk.metadata.source_file.clone();
        retriever.add_with_origin(chunk, embedding, ChunkOrigin::Attachment(name));
    }
    Ok(retriever)
}

/// Calcula la cobertura de embeddings de una carpeta.
pub fn embedding_coverage(
    storage: &Storage,
    folder_id: &str,
    embedding_dimensions: usize,
) -> Result<EmbeddingCoverage> {
    let chunks = storage.count_chunks(folder_id)? as usize;
    let embedded =
        storage.count_chunks_with_embedding_dimensions(folder_id, embedding_dimensions)? as usize;
    Ok(EmbeddingCoverage { chunks, embedded })
}

#[cfg(test)]
mod tests {
    use super::*;
    use raggy_rag::ChunkMetadata;
    use raggy_storage::{Folder, FolderStatus, IndexOwner};

    fn chunk(id: &str, content: &str) -> raggy_rag::Chunk {
        raggy_rag::Chunk {
            id: id.to_string(),
            content_hash: id.to_string(),
            content: content.to_string(),
            metadata: ChunkMetadata {
                source_file: "guia.md".to_string(),
                source_path: "guia.md".to_string(),
                heading: None,
                location: None,
                start_line: 1,
                end_line: 1,
            },
        }
    }

    #[test]
    fn reutiliza_el_indice_hasta_que_cambia_la_revision_de_la_carpeta() {
        let mut storage = Storage::open_in_memory().expect("abre SQLite");
        let folder = Folder::new("/tmp/busqueda", "búsqueda");
        storage.upsert_folder(&folder).expect("guarda carpeta");
        storage
            .replace_chunks(
                IndexOwner::Folder(&folder.id),
                "guia.md",
                &[chunk("primero", "alfa")],
                &[vec![0.0; 32]],
            )
            .expect("guarda chunk inicial");

        let mut cache = SearchCache::default();
        let options = RetrieverOptions {
            top_k: 5,
            relative_floor: 0.01,
        };
        let initial = cache
            .search_folder(&storage, &folder.id, "alfa", &options, 32)
            .expect("busca el índice inicial");
        assert_eq!(initial.len(), 1);

        storage
            .replace_chunks(
                IndexOwner::Folder(&folder.id),
                "guia.md",
                &[chunk("segundo", "beta")],
                &[vec![0.0; 32]],
            )
            .expect("cambia chunks sin cerrar la indexación");
        let cached = cache
            .search_folder(&storage, &folder.id, "beta", &options, 32)
            .expect("busca el índice cacheado");
        assert!(cached.is_empty(), "no reconstruye en cada consulta");

        storage
            .set_folder_status(&folder.id, FolderStatus::Indexed, 1)
            .expect("publica una nueva revisión de índice");
        let refreshed = cache
            .search_folder(&storage, &folder.id, "beta", &options, 32)
            .expect("reconstruye tras la reindexación");
        assert_eq!(refreshed.len(), 1);
        assert_eq!(refreshed[0].chunk.id, "segundo");
    }

    fn adjunto(id: &str, name: &str, content: &str) -> raggy_rag::Chunk {
        let mut chunk = chunk(id, content);
        chunk.metadata.source_file = name.to_string();
        chunk.metadata.source_path = name.to_string();
        chunk
    }

    /// Prepara carpeta, sesión y un adjunto; devuelve el almacén y la sesión.
    fn chat_con_adjunto() -> (Storage, raggy_storage::Folder, raggy_storage::Session) {
        let mut storage = Storage::open_in_memory().expect("abre SQLite");
        let folder = Folder::new("/tmp/chat-adjuntos", "chat-adjuntos");
        storage.upsert_folder(&folder).expect("guarda carpeta");
        let session = raggy_storage::Session::new(&folder.id, "deepseek", "deepseek-chat");
        storage.upsert_session(&session).expect("guarda sesión");

        storage
            .replace_file_index(
                IndexOwner::Folder(&folder.id),
                "politicas.md",
                &[chunk(
                    "carpeta",
                    "el plan vacacional concede 23 dias de descanso",
                )],
                &[vec![0.1; 32]],
                &raggy_storage::Document::new(&folder.id, "politicas.md", "h", "p", 10, 0, 1),
            )
            .expect("guarda el índice de la carpeta");

        storage
            .replace_file_index(
                IndexOwner::Session(&session.id),
                "informe.pdf",
                &[adjunto(
                    "adjunto",
                    "informe.pdf",
                    "el informe del proveedor confirma 23 dias de descanso",
                )],
                &[vec![0.1; 32]],
                &raggy_storage::Document::for_session(
                    &session.id,
                    "informe.pdf",
                    "h",
                    "p",
                    10,
                    0,
                    1,
                ),
            )
            .expect("guarda el adjunto");

        (storage, folder, session)
    }

    #[test]
    fn la_busqueda_del_chat_mezcla_carpeta_y_adjuntos_con_su_origen() {
        let (storage, folder, session) = chat_con_adjunto();
        let mut cache = SearchCache::default();
        let options = RetrieverOptions {
            top_k: 5,
            relative_floor: 0.0,
        };

        let results = cache
            .search_chat(
                &storage,
                &folder.id,
                &session.id,
                "23 dias de descanso",
                &options,
                32,
            )
            .expect("busca carpeta y adjuntos");

        assert_eq!(
            results.len(),
            2,
            "deben aparecer ambos orígenes: {results:?}"
        );
        let archivo = results
            .iter()
            .find(|result| result.chunk.id == "carpeta")
            .expect("recupera la carpeta");
        assert_eq!(archivo.origin, ChunkOrigin::Folder);
        let informe = results
            .iter()
            .find(|result| result.chunk.id == "adjunto")
            .expect("recupera el adjunto");
        assert_eq!(
            informe.origin,
            ChunkOrigin::Attachment("informe.pdf".to_string())
        );
    }

    #[test]
    fn los_adjuntos_de_otra_sesion_no_entran_en_el_contexto() {
        let (mut storage, folder, session) = chat_con_adjunto();
        let otra = raggy_storage::Session::new(&folder.id, "deepseek", "deepseek-chat");
        storage.upsert_session(&otra).expect("guarda otra sesión");
        storage
            .replace_file_index(
                IndexOwner::Session(&otra.id),
                "ajeno.pdf",
                &[adjunto(
                    "ajeno",
                    "ajeno.pdf",
                    "el plan secreto concede 99 dias de descanso",
                )],
                &[vec![0.1; 32]],
                &raggy_storage::Document::for_session(&otra.id, "ajeno.pdf", "h", "p", 10, 0, 1),
            )
            .expect("guarda el adjunto ajeno");

        let mut cache = SearchCache::default();
        let results = cache
            .search_chat(
                &storage,
                &folder.id,
                &session.id,
                "plan de descanso",
                &RetrieverOptions {
                    top_k: 10,
                    relative_floor: 0.0,
                },
                32,
            )
            .expect("busca solo lo de la sesión");

        assert!(
            results.iter().all(|result| result.chunk.id != "ajeno"),
            "no debe filtrar información de otra sesión: {results:?}"
        );
        assert!(results.iter().any(|result| result.chunk.id == "adjunto"));
    }

    #[test]
    fn adjuntar_un_archivo_invalida_la_cache_del_chat() {
        let (mut storage, folder, session) = chat_con_adjunto();
        let mut cache = SearchCache::default();
        let options = RetrieverOptions {
            top_k: 5,
            relative_floor: 0.0,
        };

        let antes = cache
            .search_chat(
                &storage,
                &folder.id,
                &session.id,
                "presupuesto del hotel",
                &options,
                32,
            )
            .expect("primera búsqueda");
        assert!(
            antes.iter().all(|result| result.chunk.id != "hotel"),
            "todavía no hay contenido del hotel: {antes:?}"
        );

        storage
            .replace_file_index(
                IndexOwner::Session(&session.id),
                "hotel.pdf",
                &[adjunto(
                    "hotel",
                    "hotel.pdf",
                    "el presupuesto del hotel es de 120 euros por noche",
                )],
                &[vec![0.1; 32]],
                &raggy_storage::Document::for_session(&session.id, "hotel.pdf", "h", "p", 10, 0, 1),
            )
            .expect("adjunta un archivo nuevo");

        let despues = cache
            .search_chat(
                &storage,
                &folder.id,
                &session.id,
                "presupuesto del hotel",
                &options,
                32,
            )
            .expect("reconstruye tras adjuntar");
        assert!(
            despues.iter().any(|result| result.chunk.id == "hotel"),
            "la caché debe renovarse al adjuntar: {despues:?}"
        );
    }

    #[test]
    fn quitar_los_adjuntos_deja_solo_la_carpeta() {
        let (mut storage, folder, session) = chat_con_adjunto();
        storage
            .delete_session_index(&session.id)
            .expect("quita los adjuntos");

        let mut cache = SearchCache::default();
        let results = cache
            .search_chat(
                &storage,
                &folder.id,
                &session.id,
                "23 dias de descanso",
                &RetrieverOptions {
                    top_k: 5,
                    relative_floor: 0.0,
                },
                32,
            )
            .expect("busca sin adjuntos");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk.id, "carpeta");
        assert!(storage
            .list_session_documents(&session.id)
            .expect("lista adjuntos")
            .is_empty());
    }
}
