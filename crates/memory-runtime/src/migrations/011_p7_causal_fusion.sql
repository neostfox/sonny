-- P7-A/B: causal role on observations + do-calculus sufficient stats on edges.

ALTER TABLE observation ADD COLUMN causal_role TEXT;
-- Sufficient statistics for a directed causal edge (cause → effect):
--   p_do        ≈ P(effect | do(cause)) from Intervention evidence
--   p_given     ≈ P(effect | cause)     from any co-occurrence
--   p_not_given ≈ P(effect | ¬cause)    from contrast / negative cases
ALTER TABLE concept_relation ADD COLUMN p_do REAL;
ALTER TABLE concept_relation ADD COLUMN p_given REAL;
ALTER TABLE concept_relation ADD COLUMN p_not_given REAL;
