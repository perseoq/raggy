-- Esquema inicial de Raggy Desktop (Fase 0).
-- Entidades alineadas con las épicas 1, 2, 3 y 6.

CREATE TABLE IF NOT EXISTS folders (
    id          TEXT PRIMARY KEY,
    path        TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'pending',
    file_count  INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS chunks (
    id           TEXT PRIMARY KEY,
    folder_id    TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    source_path  TEXT NOT NULL,
    source_file  TEXT NOT NULL,
    heading      TEXT,
    start_line   INTEGER NOT NULL,
    end_line     INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    content      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_chunks_folder ON chunks(folder_id);
CREATE INDEX IF NOT EXISTS idx_chunks_source ON chunks(folder_id, source_path);
CREATE INDEX IF NOT EXISTS idx_chunks_hash ON chunks(folder_id, source_path, content_hash);

CREATE TABLE IF NOT EXISTS sessions (
    id          TEXT PRIMARY KEY,
    folder_id   TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    title       TEXT NOT NULL,
    provider    TEXT NOT NULL,
    model       TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sessions_folder ON sessions(folder_id, updated_at DESC);

CREATE TABLE IF NOT EXISTS messages (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    role        TEXT NOT NULL,
    content     TEXT NOT NULL,
    anchored    INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, created_at);

CREATE TABLE IF NOT EXISTS artifacts (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    path        TEXT NOT NULL,
    model       TEXT,
    created_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_artifacts_session ON artifacts(session_id);
