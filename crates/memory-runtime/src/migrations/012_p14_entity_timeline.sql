-- P14: Entity–Property–Time timeline.
-- Same (entity, property) forms a versioned history instead of overwrite.
-- Only one row may be 'active'; older rows keep provenance and valid_to.

CREATE TABLE IF NOT EXISTS entity_property_timeline (
    entry_id       TEXT NOT NULL,
    workspace_id   TEXT NOT NULL,
    entity         TEXT NOT NULL,
    property       TEXT NOT NULL,
    value          TEXT,
    status         TEXT NOT NULL DEFAULT 'active', -- active | expired | superseded
    observation_id TEXT,
    valid_from     TEXT NOT NULL,
    valid_to       TEXT,
    superseded_by  TEXT,
    created_at     TEXT NOT NULL,
    PRIMARY KEY (entry_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_timeline_one_active
    ON entity_property_timeline(workspace_id, entity, property)
    WHERE status = 'active';

CREATE INDEX IF NOT EXISTS idx_timeline_scope
    ON entity_property_timeline(workspace_id, entity, property, valid_from);
