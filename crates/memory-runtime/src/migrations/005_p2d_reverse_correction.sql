-- P2-D: reverse-correction mechanism.
-- Problem solved: the extract pipeline was one-way — a stale extraction prompt left
-- wrong Observations in the store with no way to replace them.
--
-- 1. raw_memory.extraction_version records the prompt version used by the last
--    extraction over that memory's session. `reextract` compares it against the
--    current EXTRACTION_PROMPT_VERSION to decide whether re-extraction is needed.
-- 2. observation.superseded_by points a superseded Observation at the replacement
--    batch. The old row is kept (status = 'superseded') for traceability; it is
--    excluded from recall by the status filter.

ALTER TABLE raw_memory ADD COLUMN extraction_version TEXT;

ALTER TABLE observation ADD COLUMN superseded_by TEXT;

CREATE INDEX IF NOT EXISTS idx_obs_superseded_by ON observation(superseded_by);
