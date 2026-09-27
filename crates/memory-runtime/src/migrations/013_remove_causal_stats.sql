-- 013: remove P7 do-statistics and causal relation type.
-- Capability probe showed association-only `causes` rows were stored as causal
-- edges and production extract never populated causal_role — drop the pretence.

-- Existing causal rows were association-as-causation; fold into shared_entity.
UPDATE concept_relation SET relation_type = 'shared_entity'
WHERE relation_type = 'causal';

-- SQLite cannot DROP multiple columns in one stmt on older versions; one each.
ALTER TABLE concept_relation DROP COLUMN p_do;
ALTER TABLE concept_relation DROP COLUMN p_given;
ALTER TABLE concept_relation DROP COLUMN p_not_given;
ALTER TABLE observation DROP COLUMN causal_role;
