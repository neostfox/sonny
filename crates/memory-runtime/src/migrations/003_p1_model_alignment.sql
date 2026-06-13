-- P1 model alignment: confidence dual-dimension + MemoryItem removal.
-- observation.confidence -> extraction_confidence (immutable, source_type-derived).
-- New Observation attributes (design §4.3): memory_type_candidate + observation_detail_json.
-- MemoryItem is no longer a persisted entity.

ALTER TABLE observation RENAME COLUMN confidence TO extraction_confidence;

ALTER TABLE observation ADD COLUMN memory_type_candidate TEXT;
ALTER TABLE observation ADD COLUMN observation_detail_json TEXT;

DROP INDEX IF EXISTS idx_mi_workspace;
DROP TABLE IF EXISTS memory_item;
