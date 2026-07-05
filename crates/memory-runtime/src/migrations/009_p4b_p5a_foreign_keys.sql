-- P4-B F2 + P5-A follow-up: add FOREIGN KEY integrity to the feedback ledger
-- and the concept edge table.
--
-- Problem solved: neither table constrained its endpoints. A feedback row
-- could name a concept/observation that does not exist, and a concept_relation
-- edge could point at a ghost concept (P5-A audit) — silent dangling references
-- that corrupt recall/consolidation downstream. SQLite cannot ALTER TABLE ADD
-- CONSTRAINT, so both tables are rebuilt with the FKs in their CREATE.
--
-- Foreign keys are enforced (PRAGMA foreign_keys = ON, set at connection open).
-- Migrations run on a freshly-created database before any row is inserted, so
-- the data-preserving copy below moves zero rows in practice; it is written
-- data-safely for the file-backed case where 007/008 already carried rows.
-- (PRAGMA foreign_keys cannot be toggled inside the migration's transaction, so
-- the copy relies on existing rows already satisfying the constraints.)

-- feedback (from 008): empty at migration time — drop + recreate with FKs.
DROP TABLE IF EXISTS feedback;

CREATE TABLE feedback (
    feedback_id    TEXT NOT NULL PRIMARY KEY,
    workspace_id   TEXT NOT NULL,
    concept_id     TEXT NOT NULL REFERENCES concept(concept_id),
    observation_id TEXT REFERENCES observation(observation_id),
    feedback_type  TEXT NOT NULL,
    feedback_text  TEXT NOT NULL,
    alpha_delta    REAL NOT NULL DEFAULT 0.0,
    beta_delta     REAL NOT NULL DEFAULT 0.0,
    created_at     TEXT NOT NULL
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_feedback_concept
    ON feedback(workspace_id, concept_id);

-- concept_relation (from 007): rebuild preserving rows, adding endpoint FKs.
ALTER TABLE concept_relation RENAME TO concept_relation_old;

CREATE TABLE concept_relation (
    relation_id      TEXT NOT NULL,
    workspace_id     TEXT NOT NULL,
    src_concept_id   TEXT NOT NULL REFERENCES concept(concept_id),
    dst_concept_id   TEXT NOT NULL REFERENCES concept(concept_id),
    relation_type    TEXT NOT NULL,
    lifecycle        TEXT NOT NULL DEFAULT 'candidate',
    evidence_alpha   REAL NOT NULL DEFAULT 1.0,
    evidence_beta    REAL NOT NULL DEFAULT 1.0,
    evidence_count   INTEGER NOT NULL DEFAULT 0,
    last_evidence_at TEXT,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL,
    PRIMARY KEY (workspace_id, src_concept_id, dst_concept_id, relation_type)
) WITHOUT ROWID;

INSERT INTO concept_relation SELECT * FROM concept_relation_old;
DROP TABLE concept_relation_old;

CREATE INDEX IF NOT EXISTS idx_relation_src
    ON concept_relation(workspace_id, src_concept_id);
CREATE INDEX IF NOT EXISTS idx_relation_dst
    ON concept_relation(workspace_id, dst_concept_id);
