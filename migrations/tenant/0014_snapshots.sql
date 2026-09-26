-- ============================================================================
-- 0014: Snapshots
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 5.2.41, 5.16
--
-- The control record, the preparation state and the status notification
-- outbox of every Snapshot. A Snapshot's captured data is not here and is not
-- tagged onto the live relations: each Snapshot owns a PostgreSQL schema
-- holding its own copy of the branch relations, named from its
-- `snapshot_key`, `snap_<key>` for the published branch and `snap_<key>_stg`
-- for the private preparation branch. Because the name is a pure function of
-- the key, no branch pointer is stored. Reaping a Snapshot drops its schema
-- and deletes its `snapshots` row, which cascades to the rest.
--
-- Codes:
--
--   snapshot_preparation.status: 1 preparing, 2 success, 3 partial, 4 empty,
--     5 failure
-- ============================================================================

-- The control record, written once at creation and read on every scoping
-- resolution. A later metadata update (clause 5.16.4) rewrites only
-- `snapshot_priority` and `expires_at`; the lease and per-run results live in
-- `snapshot_preparation`, so a lease renewal never rewrites the query
-- documents.
CREATE TABLE snapshots (
    -- Names the branch schemas and keys the child rows, where the URI would
    -- repeat in every branch relation.
    snapshot_key BIGINT GENERATED ALWAYS AS IDENTITY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    snapshot_priority SMALLINT NOT NULL DEFAULT 5,
    -- The Snapshot an asynchronous clone copies from; NULL for a capture by
    -- query.
    clone_source_id TEXT COMPRESSION lz4 COLLATE "C",
    id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    snapshot_lifetime TEXT COMPRESSION lz4,
    endpoint TEXT COMPRESSION lz4,
    snapshot_queries JSONB COMPRESSION lz4,
    snapshot_temporal_queries JSONB COMPRESSION lz4,
    -- The resolved JSON-LD context both query families are compiled and run
    -- under, stored so that a replica preparing the branch never falls back
    -- to its own ambient context.
    query_context JSONB COMPRESSION lz4 NOT NULL,
    receiver_info JSONB COMPRESSION lz4,
    CONSTRAINT pk_snapshots PRIMARY KEY (snapshot_key),
    CONSTRAINT uq_snapshots_id UNIQUE (id),
    CONSTRAINT chk_snapshots_priority CHECK (snapshot_priority BETWEEN 1 AND 10),
    CONSTRAINT chk_snapshots_id_length CHECK (octet_length(id) <= 1024),
    CONSTRAINT chk_snapshots_clone_source_id_length CHECK (octet_length(clone_source_id) <= 1024)
);

CREATE INDEX idx_snapshots_expires_at ON snapshots (expires_at);

-- The lifecycle and lease row. Every claim, lease renewal, status transition
-- and `last_used_at` touch rewrites this row and nothing else.
CREATE TABLE snapshot_preparation (
    snapshot_key BIGINT NOT NULL,
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ,
    claimed_at TIMESTAMPTZ,
    lease_until TIMESTAMPTZ,
    claimed_by UUID,
    -- The highest branch migration the Snapshot's schemas have been brought
    -- to. Below the version the running broker carries, the upgrade has not
    -- run or has failed, and the Snapshot is quarantined: it is refused for
    -- scoping, cloning and notification delivery while the tenant keeps
    -- serving, and expiry, deletion and quota eviction still reap it.
    branch_schema_version INTEGER NOT NULL,
    status SMALLINT NOT NULL,
    -- Why the last branch upgrade failed; NULL once the branch is current.
    branch_upgrade_error TEXT COMPRESSION lz4,
    queries_details JSONB COMPRESSION lz4,
    temporal_queries_details JSONB COMPRESSION lz4,
    CONSTRAINT pk_snapshot_preparation PRIMARY KEY (snapshot_key),
    CONSTRAINT fk_snapshot_preparation_snapshot FOREIGN KEY (snapshot_key)
        REFERENCES snapshots (snapshot_key) ON DELETE CASCADE,
    CONSTRAINT chk_snapshot_preparation_claim_coherent
        CHECK (num_nonnulls(claimed_by, claimed_at, lease_until) IN (0, 3)),
    CONSTRAINT chk_snapshot_preparation_status CHECK (status BETWEEN 1 AND 5),
    CONSTRAINT chk_snapshot_preparation_branch_schema_version_positive
        CHECK (branch_schema_version > 0),
    CONSTRAINT chk_snapshot_preparation_branch_upgrade_error_length
        CHECK (branch_upgrade_error IS NULL OR char_length(branch_upgrade_error) <= 2048)
) WITH (
    fillfactor = 50,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 100,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 100,
    autovacuum_vacuum_cost_delay = 0
);

-- The preparation claim: preparing Snapshots whose lease has lapsed. The
-- priority and creation order it sorts by are joined in from `snapshots`.
CREATE INDEX idx_snapshot_preparation_claim
    ON snapshot_preparation (lease_until)
    WHERE status = 1;

-- Status notifications are lifecycle events, not subscription deliveries, so
-- they have their own outbox: no Subscription is invented to own them, and a
-- status transition commits whether or not the endpoint is reachable. The
-- table keeps default storage parameters: its rows arrive at the rate of
-- Snapshot status changes, and the Snapshot quota bounds how many exist.
CREATE TABLE snapshot_notification_outbox (
    id UUID NOT NULL,
    snapshot_key BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    claimed_at TIMESTAMPTZ,
    lease_until TIMESTAMPTZ,
    claimed_by UUID,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    delivered_at TIMESTAMPTZ,
    exhausted_at TIMESTAMPTZ,
    endpoint TEXT COMPRESSION lz4 NOT NULL,
    receiver_info JSONB COMPRESSION lz4,
    payload JSONB COMPRESSION lz4 NOT NULL,
    last_error TEXT COMPRESSION lz4,
    CONSTRAINT pk_snapshot_notification_outbox PRIMARY KEY (id),
    CONSTRAINT fk_snapshot_notification_outbox_snapshot FOREIGN KEY (snapshot_key)
        REFERENCES snapshots (snapshot_key) ON DELETE CASCADE,
    CONSTRAINT chk_snapshot_notification_outbox_attempt_count_non_negative CHECK (attempt_count >= 0),
    CONSTRAINT chk_snapshot_notification_outbox_last_error_length
        CHECK (last_error IS NULL OR char_length(last_error) <= 2048),
    CONSTRAINT chk_snapshot_notification_outbox_claim_coherent
        CHECK (num_nonnulls(claimed_at, lease_until, claimed_by) IN (0, 3)),
    CONSTRAINT chk_snapshot_notification_outbox_terminal_coherent
        CHECK (num_nonnulls(delivered_at, exhausted_at) <= 1)
);

CREATE INDEX idx_snapshot_notification_outbox_claim
    ON snapshot_notification_outbox
        (GREATEST(next_attempt_at, COALESCE(lease_until, '-infinity'::timestamptz)), id)
    WHERE delivered_at IS NULL AND exhausted_at IS NULL;

CREATE INDEX idx_snapshot_notification_outbox_snapshot
    ON snapshot_notification_outbox (snapshot_key);
