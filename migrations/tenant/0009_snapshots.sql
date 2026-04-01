CREATE TABLE snapshots (
    id TEXT NOT NULL PRIMARY KEY,
    snapshot_queries JSONB,
    snapshot_temporal_queries JSONB,
    snapshot_lifetime TEXT,
    snapshot_priority SMALLINT NOT NULL DEFAULT 5 CHECK (snapshot_priority BETWEEN 1 AND 10),
    endpoint TEXT,
    receiver_info JSONB,
    status TEXT NOT NULL DEFAULT 'preparing' CHECK (status IN ('preparing', 'success', 'partial', 'empty', 'failure')),
    queries_details JSONB,
    temporal_queries_details JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    last_used_at TIMESTAMPTZ
);

-- Efficient periodic cleanup of expired snapshots
CREATE INDEX idx_snapshots_expires_at ON snapshots(expires_at);

CREATE TABLE snapshot_entities (
    snapshot_id TEXT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    entity_id TEXT NOT NULL,
    entity_data JSONB NOT NULL,
    PRIMARY KEY (snapshot_id, entity_id)
);

CREATE TABLE snapshot_temporal_entities (
    snapshot_id TEXT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    entity_id TEXT NOT NULL,
    temporal_data JSONB NOT NULL,
    PRIMARY KEY (snapshot_id, entity_id)
);
