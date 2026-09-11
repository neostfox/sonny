-- P6-A/B: Concept lifecycle scope + Observation cross-project count.
-- Project-scoped knowledge stays private. Domain/Global concepts become
-- visible to other workspaces once promotion thresholds are met.
-- cross_project_count records how many distinct workspaces independently
-- observed the same (subject, predicate, object) triple.

ALTER TABLE concept ADD COLUMN lifecycle_scope TEXT NOT NULL DEFAULT 'project';
ALTER TABLE concept ADD COLUMN scope_key TEXT;

ALTER TABLE observation ADD COLUMN cross_project_count INTEGER NOT NULL DEFAULT 1;

-- P6-E: after promotion, peer project concepts that are the same knowledge
-- are linked as aliases so recall can resolve them to the primary concept.
CREATE TABLE IF NOT EXISTS concept_alias (
    alias_id           TEXT NOT NULL,
    primary_concept_id TEXT NOT NULL,
    alias_concept_id   TEXT NOT NULL UNIQUE,
    reason             TEXT,
    created_at         TEXT NOT NULL,
    PRIMARY KEY (alias_id)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_concept_alias_primary
    ON concept_alias(primary_concept_id);

CREATE INDEX IF NOT EXISTS idx_concept_scope
    ON concept(lifecycle_scope, scope_key);
