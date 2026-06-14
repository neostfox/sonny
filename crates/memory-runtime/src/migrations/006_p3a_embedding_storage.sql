-- P3-A: BLOB-backed embedding storage.
--
-- This is the pure-Rust fallback/working default: vectors are stored as a
-- little-endian f32 byte stream in `vector`, then searched by loading workspace
-- rows and computing cosine similarity in Rust. sqlite-vec can replace this later
-- as a performance optimization behind the existing `sqlite-vec` feature.

CREATE TABLE IF NOT EXISTS embedding (
    source_type   TEXT NOT NULL,
    source_id     TEXT NOT NULL,
    workspace_id  TEXT NOT NULL,
    text          TEXT,
    vector        BLOB NOT NULL,
    created_at    TEXT NOT NULL,
    PRIMARY KEY (source_type, source_id)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_embedding_workspace ON embedding(workspace_id);
