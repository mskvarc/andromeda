-- ============================================================================
-- 0017: Snapshots -- control record, preparation state, status outbox
-- ============================================================================
-- NGSI-LD Snapshot management (spec clause 5.2.22, operations 5.16.x).
--
-- A Snapshot captures a point-in-time view of entity or temporal entity data.
-- Its captured state does not live in these tables and is not tagged onto the
-- live relations: each Snapshot owns its own PostgreSQL schema `snap_<key>`
-- holding the identical branch relation set, and the live branch is the
-- tenant's default schema. The schema name is a pure function of the Snapshot's
-- surrogate `snapshot_key` (`snap_<key>` for the published branch,
-- `snap_<key>_stg` for the private preparation branch), so no branch pointer is
-- stored here. Reaping a Snapshot drops its schema; there is no row-level
-- cascade over the live relations.
--
-- The control record is split from the preparation state by write pattern, the
-- same discipline `entities` -> `entity_documents`/`entity_search` and
-- `subscriptions` -> `subscription_delivery_state` follow:
--
--   * `snapshots` is the immutable control record: the surrogate key the
--     schema is named from, the API identity, the resolved query definitions
--     and JSON-LD context, the notification endpoint, the clone source, and the
--     lifetime bounds. It is written once at creation and read on every scoping
--     resolution; the scheduling priority and expiry are the only members a
--     later metadata patch (clause 5.16.x) rewrites. It carries no lease and no
--     per-run result detail, so a preparation lease tick never rewrites its
--     wide JSONB payload.
--   * `snapshot_preparation` is the narrow lifecycle and lease row, keyed by
--     `snapshot_key`: the status machine, the renewable worker claim, the
--     modification and last-used instants, and the per-run query result
--     details. Every claim, lease renewal and status transition rewrites this
--     row and nothing else, so it takes `fillfactor = 50` and the queue-shaped
--     autovacuum cadence and its updates stay heap-only.
--
-- Lifecycle: preparing -> success/partial/empty/failure. Expired snapshots are
-- reaped by a periodic worker on `expires_at`; the reap drops the branch schema
-- and deletes the `snapshots` row, cascading its preparation and outbox rows.
--
-- Physical shape:
--
--   * `snapshot_key` is a database-issued surrogate. A snapshot's URI is long
--     and would repeat once per branch relation were the branch keyed by it, so
--     the surrogate names both the schema and the child rows. The URI stays the
--     API identity (`id`, unique).
--   * `id` and `clone_source_id` are identifiers, compared for equality and
--     prefix only, so they are `COLLATE "C"`.
--   * Every JSONB column is `COMPRESSION lz4`: a snapshot document is written
--     once and read a few times, and lz4 decompresses far faster than pglz.
--   * Not-null fixed-width columns lead, in descending alignment, then the
--     variable-length columns follow.
-- ============================================================================

CREATE TABLE snapshots (
    -- Surrogate the branch schema and child rows are named from; never rendered.
    snapshot_key BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,

    -- When this snapshot was created.
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- When this snapshot expires and should be reaped.
    expires_at TIMESTAMPTZ NOT NULL,

    -- Priority hint (1-10, default 5) for snapshot scheduling.
    snapshot_priority SMALLINT NOT NULL DEFAULT 5 CHECK (snapshot_priority BETWEEN 1 AND 10),

    -- Source URI for an asynchronous clone job; NULL for query capture.
    clone_source_id TEXT COMPRESSION lz4 COLLATE "C",

    -- Snapshot URI, e.g. "urn:ngsi-ld:Snapshot:abc123".
    id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL UNIQUE,

    -- ISO 8601 duration for how long the snapshot remains valid.
    snapshot_lifetime TEXT,

    -- Optional notification endpoint for snapshot completion.
    endpoint TEXT,

    -- Entity query parameters used to produce this snapshot.
    snapshot_queries JSONB COMPRESSION lz4,

    -- Temporal query parameters used to produce this snapshot.
    snapshot_temporal_queries JSONB COMPRESSION lz4,

    -- Resolved JSON-LD context used to compile and execute both query families.
    -- Stored with the control record so another replica does not fall back to
    -- its own ambient context while preparing the branch.
    query_context JSONB COMPRESSION lz4 NOT NULL,

    -- Optional receiver info for the notification endpoint.
    receiver_info JSONB COMPRESSION lz4
);

-- Efficient periodic cleanup of expired snapshots.
CREATE INDEX idx_snapshots_expires_at ON snapshots (expires_at);

CREATE TABLE snapshot_preparation (
    -- Shares the control record's surrogate key and is reaped with it.
    snapshot_key BIGINT PRIMARY KEY REFERENCES snapshots (snapshot_key) ON DELETE CASCADE,

    -- Modification instant, bumped on every lifecycle transition.
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Last time the snapshot was accessed.
    last_used_at TIMESTAMPTZ,

    -- Renewable preparation lease. All three values are present together or
    -- absent together; an expired lease is reclaimable by another replica.
    claimed_at TIMESTAMPTZ,
    lease_until TIMESTAMPTZ,
    claimed_by UUID,

    -- Snapshot lifecycle state.
    status snapshot_status NOT NULL DEFAULT 'preparing',

    -- Per-query result details for entity queries.
    queries_details JSONB COMPRESSION lz4,

    -- Per-query result details for temporal queries.
    temporal_queries_details JSONB COMPRESSION lz4,

    CONSTRAINT chk_snapshot_preparation_claim_coherent
        CHECK (num_nonnulls(claimed_by, claimed_at, lease_until) IN (0, 3))
) WITH (
    -- Updated in place on every status transition, lease tick and `last_used_at`
    -- touch, so leave half of every page free for heap-only tuple updates and
    -- vacuum on the update rate rather than the default scale factors.
    fillfactor = 50,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 100,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 100,
    autovacuum_vacuum_cost_delay = 0
);

-- The preparation claim finds the highest-priority preparing snapshot whose
-- lease has lapsed. The partial index keeps only preparing rows; the priority
-- and creation order it sorts by live on the control record, joined in.
CREATE INDEX idx_snapshot_preparation_claim
    ON snapshot_preparation (lease_until)
    WHERE status = 'preparing';

-- Snapshot status notifications are lifecycle events, not subscription
-- deliveries. Keeping their own outbox avoids inventing a Subscription FK and
-- lets state promotion commit independently of endpoint availability. The lease
-- and dead-letter columns are the schema-wide claimable-queue contract:
-- `claimed_at`/`claimed_by`/`lease_until` are the renewable claim,
-- `attempt_count` the counter, `next_attempt_at` the next-due instant,
-- `last_error` the latest failure, `delivered_at` the acknowledged door and
-- `exhausted_at` the dead-letter door.
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
