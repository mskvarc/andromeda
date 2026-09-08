-- ============================================================================
-- 0017: Snapshots
-- ============================================================================
-- NGSI-LD Snapshot management (spec clause 5.2.22, operations 5.16.x).
--
-- A Snapshot captures a point-in-time view of entity or temporal entity data.
-- `snapshots` is the control record; captured state lives in the ordinary
-- context-space-partitioned current and temporal relations.
--
-- Lifecycle: preparing -> success/partial/empty/failure. Expired snapshots are
-- reaped by a periodic worker on `expires_at`, and branch rows go with the
-- context-space parent through cascading foreign keys.
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

    -- First-class context branch owned by this control record.
    context_space_key BIGINT NOT NULL UNIQUE REFERENCES context_spaces (context_space_key) ON DELETE CASCADE,

    -- Private branch populated by capture and clone workers. Promotion atomically makes
    -- it the Snapshot's `context_space_key` and deletes the original empty
    -- branch. It is never resolved by HTTP scoping before that swap.
    staging_context_space_key BIGINT UNIQUE REFERENCES context_spaces (context_space_key) ON DELETE SET NULL,

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

    -- Renewable preparation lease. All three values are present together or
    -- absent together; an expired lease is reclaimable by another replica.
    claimed_by UUID,
    claimed_at TIMESTAMPTZ,
    lease_until TIMESTAMPTZ,

    -- Source URI for an asynchronous clone job; NULL for query capture.
    clone_source_id TEXT COMPRESSION lz4 COLLATE "C",

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

    -- Resolved JSON-LD context used to compile and execute both query families.
    -- Stored with the control job so another replica does not fall back to its
    -- own ambient context while preparing the branch.
    query_context JSONB COMPRESSION lz4 NOT NULL,

    -- Optional receiver info for the notification endpoint
    receiver_info JSONB COMPRESSION lz4,

    -- Per-query result details for entity queries
    queries_details JSONB COMPRESSION lz4,

    -- Per-query result details for temporal queries
    temporal_queries_details JSONB COMPRESSION lz4,

    CONSTRAINT chk_snapshot_claim_coherent
        CHECK (num_nonnulls(claimed_by, claimed_at, lease_until) IN (0, 3))
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

CREATE INDEX idx_snapshots_preparation_claim
    ON snapshots (lease_until, snapshot_priority DESC, created_at)
    WHERE status = 'preparing';

-- Snapshot status notifications are lifecycle events, not subscription
-- deliveries. Keeping their own outbox avoids inventing a Subscription FK and
-- lets state promotion commit independently of endpoint availability.
CREATE TABLE snapshot_notification_outbox (
    id UUID PRIMARY KEY,
    snapshot_key BIGINT NOT NULL REFERENCES snapshots(snapshot_key) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    claimed_at TIMESTAMPTZ,
    lease_until TIMESTAMPTZ,
    claimed_by UUID,
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    delivered_at TIMESTAMPTZ,
    exhausted_at TIMESTAMPTZ,
    endpoint TEXT NOT NULL,
    receiver_info JSONB COMPRESSION lz4,
    payload JSONB COMPRESSION lz4 NOT NULL,
    last_error TEXT COMPRESSION lz4,
    CONSTRAINT chk_snapshot_notification_claim_coherent
        CHECK (num_nonnulls(claimed_at, lease_until, claimed_by) IN (0, 3)),
    CONSTRAINT chk_snapshot_notification_terminal_coherent
        CHECK (num_nonnulls(delivered_at, exhausted_at) <= 1)
) WITH (
    fillfactor = 50,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 100
);

CREATE INDEX idx_snapshot_notification_claimable
    ON snapshot_notification_outbox
        (GREATEST(next_attempt_at, COALESCE(lease_until, '-infinity'::timestamptz)), id)
    WHERE delivered_at IS NULL AND exhausted_at IS NULL;
