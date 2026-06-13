-- P2-C: Observation co-occurrence (coclaim) relationships.
-- Problem solved: Observations had no inter-relationships, so clustering had to
-- infer affinity from zero. An extraction batch is the strongest co-occurrence
-- signal: observations claimed together in one extract_observations() call.
--
-- 1. extraction_batch_id groups observations from a single extraction call.
-- 2. observation_coclaim records each unordered pair within a batch, so the
--    cluster engine (P3-B) can score co-claimed affinity without re-scanning.

ALTER TABLE observation ADD COLUMN extraction_batch_id TEXT;

CREATE INDEX IF NOT EXISTS idx_obs_extraction_batch
    ON observation(extraction_batch_id);

CREATE TABLE IF NOT EXISTS observation_coclaim (
    -- Unordered pair; stored with observation_a < observation_b so the PK is canonical.
    observation_a  TEXT NOT NULL,
    observation_b  TEXT NOT NULL,
    batch_id       TEXT NOT NULL,
    workspace_id   TEXT NOT NULL,
    created_at     TEXT NOT NULL,
    PRIMARY KEY (observation_a, observation_b)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_obs_coclaim_batch ON observation_coclaim(batch_id);
CREATE INDEX IF NOT EXISTS idx_obs_coclaim_ws ON observation_coclaim(workspace_id);
