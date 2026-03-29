CREATE TABLE IF NOT EXISTS snapshots (
    id TEXT NOT NULL PRIMARY KEY,
    snapshot_queries JSONB,
    snapshot_temporal_queries JSONB,
    snapshot_lifetime TEXT,
    snapshot_priority SMALLINT NOT NULL DEFAULT 5 CHECK (snapshot_priority BETWEEN 1 AND 10),
    endpoint TEXT,
    receiver_info JSONB,
    status TEXT NOT NULL DEFAULT 'preparing',
    queries_details JSONB,
    temporal_queries_details JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    last_used_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS snapshot_entities (
    snapshot_id TEXT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    entity_id TEXT NOT NULL,
    entity_data JSONB NOT NULL,
    PRIMARY KEY (snapshot_id, entity_id)
);

CREATE TABLE IF NOT EXISTS snapshot_temporal_entities (
    snapshot_id TEXT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    entity_id TEXT NOT NULL,
    temporal_data JSONB NOT NULL,
    PRIMARY KEY (snapshot_id, entity_id)
);
