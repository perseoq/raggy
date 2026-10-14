-- Fase 1: seguimiento por archivo para la re-indexación incremental (HU-03).

CREATE TABLE IF NOT EXISTS documents (
    id            TEXT PRIMARY KEY,
    folder_id     TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    content_hash  TEXT NOT NULL,
    size_bytes    INTEGER NOT NULL,
    modified_at   INTEGER NOT NULL,
    indexed_at    TEXT NOT NULL,
    chunk_count   INTEGER NOT NULL DEFAULT 0,
    UNIQUE (folder_id, relative_path)
);

CREATE INDEX IF NOT EXISTS idx_documents_folder ON documents(folder_id);
