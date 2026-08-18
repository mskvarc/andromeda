-- ============================================================================
-- 0017: Snapshots
-- ============================================================================
-- NGSI-LD Snapshot management (spec clause 5.2.22, operations 5.16.x).
--
-- A Snapshot captures a point-in-time view of entity or temporal entity data,
-- produced by a snapshot query. `snapshots` is the control record; the
-- materialised documents live in two child tables (`snapshot_entities`,
-- `snapshot_temporal_entities`), one row per captured entity.
--
-- Lifecycle: preparing -> success/partial/empty/failure. Expired snapshots are
-- reaped by a periodic worker on `expires_at`, and the child rows go with them
-- through the cascading foreign key.
--
-- Physical shape:
--
--   * `snapshot_key` is a database-issued surrogate the child tables key on. A
--     snapshot's URI is long and repeated once per captured entity, so keying
--     the children by the URI stored it thousands of times over and made both
--     child primary keys as wide as the identifier they repeat. The URI stays
--     the API identity (`id`, unique) and every statement that starts from it
--     resolves the surrogate with one scalar subquery on the unique index.
--   * `id` and `entity_id` are identifiers, compared for equality and prefix
--     only, so they are `COLLATE "C"`: byte-wise comparison keeps the btree
--     free of any locale and makes an anchored pattern a range scan.
--   * Every JSONB column is `COMPRESSION lz4`: a snapshot document is written
--     once and read a few times, and lz4 decompresses far faster than pglz for
--     the same width.
--   * Not-null fixed-width columns lead, in descending alignment, and the
--     variable-length columns follow, so no row pays alignment padding it does
--     not have to.
-- ============================================================================

CREATE TABLE snapshots (
    -- Surrogate the child tables reference; never rendered.
    snapshot_key BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,

    -- System-managed temporal properties
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- When this snapshot expires and should be cleaned up
    expires_at TIMESTAMPTZ NOT NULL,

    -- Last time the snapshot was accessed
    last_used_at TIMESTAMPTZ,

    -- Priority hint (1-10, default 5) for snapshot scheduling
    snapshot_priority SMALLINT NOT NULL DEFAULT 5 CHECK (snapshot_priority BETWEEN 1 AND 10),

    -- Snapshot lifecycle state
    status snapshot_status NOT NULL DEFAULT 'preparing',

    -- Snapshot URI, e.g. "urn:ngsi-ld:Snapshot:abc123"
    id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL UNIQUE,

    -- ISO 8601 duration for how long the snapshot remains valid
    snapshot_lifetime TEXT,

    -- Optional notification endpoint for snapshot completion
    endpoint TEXT,

    -- Entity query parameters used to produce this snapshot
    snapshot_queries JSONB COMPRESSION lz4,

    -- Temporal query parameters used to produce this snapshot
    snapshot_temporal_queries JSONB COMPRESSION lz4,

    -- Optional receiver info for the notification endpoint
    receiver_info JSONB COMPRESSION lz4,

    -- Per-query result details for entity queries
    queries_details JSONB COMPRESSION lz4,

    -- Per-query result details for temporal queries
    temporal_queries_details JSONB COMPRESSION lz4
) WITH (
    -- The control row is updated in place on every status transition and on
    -- `last_used_at`, so leave half of every page free for heap-only tuple
    -- updates and vacuum on the update rate rather than the default scale
    -- factors. The child tables are insert-only and keep the server defaults.
    fillfactor = 50,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 100,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 100,
    autovacuum_vacuum_cost_delay = 0
);

-- Efficient periodic cleanup of expired snapshots
CREATE INDEX idx_snapshots_expires_at ON snapshots (expires_at);

CREATE TABLE snapshot_entities (
    -- Parent snapshot
    snapshot_key BIGINT NOT NULL REFERENCES snapshots (snapshot_key) ON DELETE CASCADE,

    -- Entity that was captured in this snapshot
    entity_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    -- Full entity payload at the time of capture
    entity_data JSONB COMPRESSION lz4 NOT NULL,

    PRIMARY KEY (snapshot_key, entity_id)
);

CREATE TABLE snapshot_temporal_entities (
    -- Parent snapshot
    snapshot_key BIGINT NOT NULL REFERENCES snapshots (snapshot_key) ON DELETE CASCADE,

    -- Entity that was captured in this snapshot
    entity_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,

    -- Full temporal entity data at the time of capture
    temporal_data JSONB COMPRESSION lz4 NOT NULL,

    PRIMARY KEY (snapshot_key, entity_id)
);
