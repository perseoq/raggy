-- Fase 5a (v2.2.5): los fragmentos y documentos pueden pertenecer a la carpeta
-- de un chat (índice normal) o a la propia sesión (archivos adjuntos al chat).
-- Un registro pertenece exactamente a uno de los dos, nunca a ambos ni a
-- ninguno, y se elimina en cascada con su propietario.
--
-- SQLite no permite quitar `NOT NULL` ni añadir `CHECK` con `ALTER TABLE`, así
-- que ambas tablas se reconstruyen conservando todas sus filas.

-- Fragmentos: `folder_id` pasa a ser opcional y se añade `session_id`.
CREATE TABLE chunks_new (
    id           TEXT PRIMARY KEY,
    folder_id    TEXT REFERENCES folders(id) ON DELETE CASCADE,
    session_id   TEXT REFERENCES sessions(id) ON DELETE CASCADE,
    source_path  TEXT NOT NULL,
    source_file  TEXT NOT NULL,
    heading      TEXT,
    location     TEXT,
    start_line   INTEGER NOT NULL,
    end_line     INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    content      TEXT NOT NULL,
    embedding    BLOB,
    CHECK ((folder_id IS NOT NULL) <> (session_id IS NOT NULL))
);

INSERT INTO chunks_new
    (id, folder_id, source_path, source_file, heading, location, start_line, end_line,
     content_hash, content, embedding)
SELECT id, folder_id, source_path, source_file, heading, location, start_line, end_line,
       content_hash, content, embedding
FROM chunks;

DROP TABLE chunks;
ALTER TABLE chunks_new RENAME TO chunks;

CREATE INDEX IF NOT EXISTS idx_chunks_folder ON chunks(folder_id);
CREATE INDEX IF NOT EXISTS idx_chunks_source ON chunks(folder_id, source_path);
CREATE INDEX IF NOT EXISTS idx_chunks_hash ON chunks(folder_id, source_path, content_hash);
CREATE INDEX IF NOT EXISTS idx_chunks_session ON chunks(session_id);
CREATE INDEX IF NOT EXISTS idx_chunks_session_source ON chunks(session_id, source_path);

-- Documentos: igual que los fragmentos.
--
-- Se conserva `UNIQUE (folder_id, relative_path)` como restricción de tabla
-- porque SQLite trata cada `NULL` como distinto: no estorba a los documentos de
-- sesión (que llevan `folder_id` a `NULL`). Para estos últimos se usa un índice
-- único parcial sobre `(session_id, relative_path)`.
CREATE TABLE documents_new (
    id              TEXT PRIMARY KEY,
    folder_id       TEXT REFERENCES folders(id) ON DELETE CASCADE,
    session_id      TEXT REFERENCES sessions(id) ON DELETE CASCADE,
    relative_path   TEXT NOT NULL,
    content_hash    TEXT NOT NULL,
    processing_hash TEXT NOT NULL DEFAULT '',
    size_bytes      INTEGER NOT NULL,
    modified_at     INTEGER NOT NULL,
    indexed_at      TEXT NOT NULL,
    chunk_count     INTEGER NOT NULL DEFAULT 0,
    UNIQUE (folder_id, relative_path),
    CHECK ((folder_id IS NOT NULL) <> (session_id IS NOT NULL))
);

INSERT INTO documents_new
    (id, folder_id, relative_path, content_hash, processing_hash, size_bytes, modified_at,
     indexed_at, chunk_count)
SELECT id, folder_id, relative_path, content_hash, processing_hash, size_bytes, modified_at,
       indexed_at, chunk_count
FROM documents;

DROP TABLE documents;
ALTER TABLE documents_new RENAME TO documents;

CREATE INDEX IF NOT EXISTS idx_documents_folder ON documents(folder_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_documents_session_path
    ON documents(session_id, relative_path) WHERE session_id IS NOT NULL;
