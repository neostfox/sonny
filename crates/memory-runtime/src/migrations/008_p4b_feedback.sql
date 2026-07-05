-- P4-B: user feedback ledger (D9).
-- Problem solved: feedback had a model but no table — user reactions ("不对",
-- "没错") never flowed back into confidence, and recall counted every retrieval
-- as successful, so hot concepts decayed ever slower without any user
-- validation (rehearsal positive-feedback defect, P4-C audit). Each row is one
-- immutable feedback event; the applied alpha/beta deltas are denormalized so
-- the confidence trajectory can be audited without replaying classification.

CREATE TABLE IF NOT EXISTS feedback (
    feedback_id    TEXT NOT NULL PRIMARY KEY,
    workspace_id   TEXT NOT NULL,
    concept_id     TEXT NOT NULL,
    observation_id TEXT,
    feedback_type  TEXT NOT NULL,
    feedback_text  TEXT NOT NULL,
    alpha_delta    REAL NOT NULL DEFAULT 0.0,
    beta_delta     REAL NOT NULL DEFAULT 0.0,
    created_at     TEXT NOT NULL
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_feedback_concept
    ON feedback(workspace_id, concept_id);
