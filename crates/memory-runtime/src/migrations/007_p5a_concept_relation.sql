-- P5-A: Concept-to-concept edges with edge-level Beta evidence.
-- Problem solved: concepts had no edges at all — spreading activation had
-- nothing to spread over, and causal knowledge extracted as `causes`
-- observations was invisible at the concept level. This table is the graph
-- substrate for recall (activation propagation) and for the causal-edge
-- upgrade chain (candidate → validated → confirmed).
--
-- Direction semantics: for directed relation types ('causal', 'temporal')
-- src is the cause / earlier side. Symmetric types store the pair with
-- src < dst so one unordered pair is exactly one row.

CREATE TABLE IF NOT EXISTS concept_relation (
    relation_id      TEXT NOT NULL,
    workspace_id     TEXT NOT NULL,
    src_concept_id   TEXT NOT NULL,
    dst_concept_id   TEXT NOT NULL,
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

CREATE INDEX IF NOT EXISTS idx_relation_src
    ON concept_relation(workspace_id, src_concept_id);
CREATE INDEX IF NOT EXISTS idx_relation_dst
    ON concept_relation(workspace_id, dst_concept_id);
