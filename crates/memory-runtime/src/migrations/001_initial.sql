CREATE TABLE IF NOT EXISTS raw_memory (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_id       TEXT UNIQUE NOT NULL,
    workspace_id    TEXT NOT NULL,
    session_id      TEXT NOT NULL,
    role            TEXT NOT NULL,
    content         TEXT NOT NULL,
    source_type     TEXT NOT NULL,
    source_ref      TEXT,
    created_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_raw_memory_workspace ON raw_memory(workspace_id);
CREATE INDEX IF NOT EXISTS idx_raw_memory_session ON raw_memory(session_id);

CREATE TABLE IF NOT EXISTS observation (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    observation_id  TEXT UNIQUE NOT NULL,
    workspace_id    TEXT NOT NULL,
    memory_id       TEXT NOT NULL REFERENCES raw_memory(memory_id),
    subject_text    TEXT NOT NULL,
    subject_type    TEXT,
    predicate       TEXT NOT NULL,
    object_text     TEXT,
    object_type     TEXT,
    evidence_text   TEXT,
    confidence      REAL NOT NULL DEFAULT 0.5,
    evidence_alpha  REAL NOT NULL DEFAULT 1.0,
    evidence_beta   REAL NOT NULL DEFAULT 1.0,
    status          TEXT NOT NULL DEFAULT 'candidate',
    surprise_score  REAL NOT NULL DEFAULT 0.5,
    source_type     TEXT NOT NULL,
    consolidated    BOOLEAN NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_obs_workspace ON observation(workspace_id);
CREATE INDEX IF NOT EXISTS idx_obs_status ON observation(workspace_id, status);
CREATE INDEX IF NOT EXISTS idx_obs_subject ON observation(subject_text);

CREATE TABLE IF NOT EXISTS memory_item (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_item_id  TEXT UNIQUE NOT NULL,
    workspace_id    TEXT NOT NULL,
    memory_type     TEXT NOT NULL,
    title           TEXT,
    content         TEXT NOT NULL,
    entities_json   TEXT,
    relations_json  TEXT,
    evidence_json   TEXT,
    confidence      REAL NOT NULL DEFAULT 0.5,
    evidence_alpha  REAL NOT NULL DEFAULT 1.0,
    evidence_beta   REAL NOT NULL DEFAULT 1.0,
    status          TEXT NOT NULL DEFAULT 'candidate',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mi_workspace ON memory_item(workspace_id);

CREATE TABLE IF NOT EXISTS concept_candidate (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    candidate_id             TEXT UNIQUE NOT NULL,
    workspace_id             TEXT NOT NULL,
    name                     TEXT NOT NULL,
    summary                  TEXT,
    source_terms_json        TEXT,
    source_sessions_json     TEXT,
    source_observations_json TEXT,
    known_facts_json         TEXT,
    rejected_hypotheses_json TEXT,
    open_questions_json      TEXT,
    evidence_json            TEXT,
    evidence_count           INTEGER NOT NULL DEFAULT 0,
    confidence               REAL NOT NULL DEFAULT 0.5,
    evidence_alpha           REAL NOT NULL DEFAULT 1.0,
    evidence_beta            REAL NOT NULL DEFAULT 1.0,
    status                   TEXT NOT NULL DEFAULT 'candidate',
    last_recalled_at         TEXT,
    recall_count             INTEGER NOT NULL DEFAULT 0,
    successful_recall_count  INTEGER NOT NULL DEFAULT 0,
    failed_recall_count      INTEGER NOT NULL DEFAULT 0,
    created_at               TEXT NOT NULL,
    updated_at               TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_cc_workspace ON concept_candidate(workspace_id);

CREATE TABLE IF NOT EXISTS concept (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    concept_id              TEXT UNIQUE NOT NULL,
    workspace_id            TEXT NOT NULL,
    name                    TEXT NOT NULL,
    concept_type            TEXT,
    definition              TEXT,
    related_entities_json   TEXT,
    known_facts_json        TEXT,
    rejected_hypotheses_json TEXT,
    open_questions_json     TEXT,
    evidence_json           TEXT,
    confidence              REAL NOT NULL DEFAULT 0.5,
    evidence_alpha          REAL NOT NULL DEFAULT 1.0,
    evidence_beta           REAL NOT NULL DEFAULT 1.0,
    status                  TEXT NOT NULL DEFAULT 'active',
    parent_concept_id       TEXT,
    hierarchy_depth         INTEGER NOT NULL DEFAULT 0,
    last_recalled_at        TEXT,
    recall_count            INTEGER NOT NULL DEFAULT 0,
    successful_recall_count INTEGER NOT NULL DEFAULT 0,
    failed_recall_count     INTEGER NOT NULL DEFAULT 0,
    connection_count        INTEGER NOT NULL DEFAULT 0,
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_concept_workspace ON concept(workspace_id);
CREATE INDEX IF NOT EXISTS idx_concept_parent ON concept(parent_concept_id);

CREATE TABLE IF NOT EXISTS entity_alias (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    canonical_form  TEXT NOT NULL,
    alias_form      TEXT NOT NULL,
    workspace_id    TEXT,
    confirmed       BOOLEAN NOT NULL DEFAULT 0,
    co_occurrence_count INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL,
    UNIQUE(canonical_form, alias_form)
);

CREATE INDEX IF NOT EXISTS idx_alias_canonical ON entity_alias(canonical_form);
CREATE INDEX IF NOT EXISTS idx_alias_alias ON entity_alias(alias_form);
