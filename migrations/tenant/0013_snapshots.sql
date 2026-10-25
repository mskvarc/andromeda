-- ============================================================================
-- 0013: Snapshots
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
create table snapshots (
    -- Names the branch schemas and keys the child rows, where the URI would
    -- repeat in every branch relation.
    snapshot_key bigint generated always as identity,
    created_at timestamptz not null default now(),
    expires_at timestamptz not null,
    snapshot_priority smallint not null default 5,
    -- The Snapshot an asynchronous clone copies from; NULL for a capture by
    -- query.
    clone_source_id text compression lz4 collate "C",
    id text compression lz4 collate "C" not null,
    snapshot_lifetime text compression lz4,
    endpoint text compression lz4,
    snapshot_queries json compression lz4,
    snapshot_temporal_queries json compression lz4,
    -- The resolved JSON-LD context both query families are compiled and run
    -- under, stored so that a replica preparing the branch never falls back
    -- to its own ambient context.
    query_context json compression lz4 not null,
    receiver_info json compression lz4,
    constraint pk_snapshots primary key (snapshot_key),
    constraint uq_snapshots_id unique (id),
    constraint chk_snapshots_priority check (snapshot_priority between 1 and 10),
    constraint chk_snapshots_id_length check (octet_length(id) <= 1024),
    constraint chk_snapshots_clone_source_id_length check (octet_length(clone_source_id) <= 1024)
);

create index idx_snapshots_expires_at on snapshots (expires_at);

-- The lifecycle and lease row. Every claim, lease renewal, status transition
-- and `last_used_at` touch rewrites this row and nothing else.
create table snapshot_preparation (
    snapshot_key bigint not null,
    modified_at timestamptz not null default now(),
    last_used_at timestamptz,
    claimed_at timestamptz,
    lease_until timestamptz,
    claimed_by uuid,
    -- The highest branch migration the Snapshot's schemas have been brought
    -- to. Below the version the running broker carries, the upgrade has not
    -- run or has failed, and the Snapshot is quarantined: it is refused for
    -- scoping, cloning and notification delivery while the tenant keeps
    -- serving, and expiry, deletion and quota eviction still reap it.
    branch_schema_version integer not null,
    status smallint not null,
    -- Why the last branch upgrade failed; NULL once the branch is current.
    branch_upgrade_error text compression lz4,
    queries_details json compression lz4,
    temporal_queries_details json compression lz4,
    constraint pk_snapshot_preparation primary key (snapshot_key),
    constraint fk_snapshot_preparation_snapshot foreign key (snapshot_key) references snapshots (snapshot_key) on delete cascade,
    constraint chk_snapshot_preparation_claim_coherent check (num_nonnulls (claimed_by, claimed_at, lease_until) in (0, 3)),
    constraint chk_snapshot_preparation_status check (status between 1 and 5),
    constraint chk_snapshot_preparation_branch_schema_version_positive check (branch_schema_version > 0),
    constraint chk_snapshot_preparation_branch_upgrade_error_length check (branch_upgrade_error is null or char_length(branch_upgrade_error) <= 2048)
)
with (
    vacuum_truncate = false,
    fillfactor = 50,
    autovacuum_vacuum_scale_factor = 0,
    autovacuum_vacuum_threshold = 100,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 100,
    autovacuum_vacuum_cost_delay = 0
);

-- The preparation claim: preparing Snapshots whose lease has lapsed. The
-- priority and creation order it sorts by are joined in from `snapshots`.
create index idx_snapshot_preparation_claim on snapshot_preparation (lease_until)
where
    status = 1;

-- Status notifications are lifecycle events, not subscription deliveries, so
-- they have their own outbox: no Subscription is invented to own them, and a
-- status transition commits whether or not the endpoint is reachable. The
-- table keeps default storage parameters: its rows arrive at the rate of
-- Snapshot status changes, and the Snapshot quota bounds how many exist.
create table snapshot_notification_outbox (
    id uuid not null,
    snapshot_key bigint not null,
    created_at timestamptz not null default now(),
    next_attempt_at timestamptz not null default now(),
    claimed_at timestamptz,
    lease_until timestamptz,
    claimed_by uuid,
    attempt_count integer not null default 0,
    delivered_at timestamptz,
    exhausted_at timestamptz,
    endpoint text compression lz4 not null,
    receiver_info json compression lz4,
    payload json compression lz4 not null,
    last_error text compression lz4,
    constraint pk_snapshot_notification_outbox primary key (id),
    constraint fk_snapshot_notification_outbox_snapshot foreign key (snapshot_key) references snapshots (snapshot_key) on delete cascade,
    constraint chk_snapshot_notification_outbox_attempt_count_non_negative check (attempt_count >= 0),
    constraint chk_snapshot_notification_outbox_last_error_length check (last_error is null or char_length(last_error) <= 2048),
    constraint chk_snapshot_notification_outbox_claim_coherent check (num_nonnulls (claimed_at, lease_until, claimed_by) in (0, 3)),
    constraint chk_snapshot_notification_outbox_terminal_coherent check (num_nonnulls (delivered_at, exhausted_at) <= 1)
);

create index idx_snapshot_notification_outbox_claim on snapshot_notification_outbox (greatest(next_attempt_at, coalesce(lease_until, '-infinity'::timestamptz)), id)
where
    delivered_at is null and exhausted_at is null;

create index idx_snapshot_notification_outbox_snapshot on snapshot_notification_outbox (snapshot_key);
