-- P0-C: entity_concept join table
-- Replaces substring LIKE search on related_entities_json with exact-match JOIN.
-- Populated by SqliteConceptStore::insert_concept / update_concept.

CREATE TABLE IF NOT EXISTS entity_concept (
    entity        TEXT NOT NULL,
    concept_id    TEXT NOT NULL,
    workspace_id  TEXT NOT NULL,
    PRIMARY KEY (entity, concept_id)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_entity_concept_ws_entity
    ON entity_concept(workspace_id, entity);
