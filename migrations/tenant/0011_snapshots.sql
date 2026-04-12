-- ============================================================================
-- 010: Snapshots
-- ============================================================================
-- NGSI-LD Snapshot management (spec clause 5.2.22).
--
-- A Snapshot captures a point-in-time view of entity or temporal entity
-- data, produced by a snapshot query. Results are stored in child tables
-- (snapshot_entities, snapshot_temporal_entities) keyed by snapshot ID.
--
-- Lifecycle: preparing -> success/partial/empty/failure.
-- Expired snapshots are cleaned up by a periodic worker using expires_at.
-- ============================================================================

CREATE TABLE snapshots (
    -- Snapshot URI, e.g. "urn:ngsi-ld:Snapshot:abc123"
    id TEXT NOT NULL PRIMARY KEY,

    -- Entity query parameters used to produce this snapshot
    snapshot_queries JSONB,

    -- Temporal query parameters used to produce this snapshot
    snapshot_temporal_queries JSONB,

    -- ISO 8601 duration for how long the snapshot remains valid
    snapshot_lifetime TEXT,

    -- Priority hint (1-10, default 5) for snapshot scheduling
    snapshot_priority SMALLINT NOT NULL DEFAULT 5 CHECK (snapshot_priority BETWEEN 1 AND 10),

    -- Optional notification endpoint for snapshot completion
    endpoint TEXT,

    -- Optional receiver info for the notification endpoint
    receiver_info JSONB,

    -- Snapshot lifecycle state
    status snapshot_status NOT NULL DEFAULT 'preparing',

    -- Per-query result details for entity queries
    queries_details JSONB,

    -- Per-query result details for temporal queries
    temporal_queries_details JSONB,

    -- System-managed temporal properties
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- When this snapshot expires and should be cleaned up
    expires_at TIMESTAMPTZ NOT NULL,

    -- Last time the snapshot was accessed
    last_used_at TIMESTAMPTZ
);

-- Efficient periodic cleanup of expired snapshots
CREATE INDEX idx_snapshots_expires_at ON snapshots(expires_at);

CREATE TABLE snapshot_entities (
    -- Parent snapshot
    snapshot_id TEXT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,

    -- Entity that was captured in this snapshot
    entity_id TEXT NOT NULL,

    -- Full entity payload at the time of capture
    entity_data JSONB NOT NULL,

    PRIMARY KEY (snapshot_id, entity_id)
);

CREATE TABLE snapshot_temporal_entities (
    -- Parent snapshot
    snapshot_id TEXT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,

    -- Entity that was captured in this snapshot
    entity_id TEXT NOT NULL,

    -- Full temporal entity data at the time of capture
    temporal_data JSONB NOT NULL,

    PRIMARY KEY (snapshot_id, entity_id)
);
