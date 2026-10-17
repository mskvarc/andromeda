-- ============================================================================
-- 0027: Pending side effects
-- ============================================================================
-- When a batch entity operation asks for `Prefer: respond-async`, the broker
-- queues here, in the same transaction as the Entity write, the changes whose
-- notifications that write did not record as durable deliveries, so either
-- both commit or neither does. The write's temporal history is queued as
-- temporal append intents in the same transaction, never here. A background
-- worker drains the queue at least once per job, and a worker that crashes
-- leaves its lease to lapse for another to reclaim. A write that owes no
-- change queues no job.
--
-- The queue exists only in the live branch. A request scoped to a Snapshot
-- runs its side effects inline, so no Snapshot branch ever has a job to
-- queue, and no worker would drain one.
-- ============================================================================
create table pending_side_effects (
    -- Minted by the broker before the statement runs, so a retried statement
    -- carries the same identity.
    id uuid not null,
    created_at timestamptz not null default now(),
    -- With `base_ordinal`, what a replay resumes from: the commit's recorded
    -- instant, and the first ordinal left free after its history intents and
    -- its notification events drew from the commit's numbering. A
    -- crash-retried job reuses both rather than minting a new identity or
    -- restarting at zero, so it never duplicates the commit's consequences.
    recorded_at timestamptz not null,
    -- One commit queues exactly one job, so the enqueue does nothing on a
    -- conflict here: a commit replayed under its original identity finds its
    -- job already queued, and the drain never runs one batch's side effects
    -- twice.
    write_id uuid not null,
    base_ordinal integer not null,
    attempt_count integer not null default 0,
    next_attempt_at timestamptz not null default now(),
    claimed_at timestamptz,
    lease_until timestamptz,
    claimed_by uuid,
    exhausted_at timestamptz,
    last_error text compression lz4,
    -- The owed changes, in the order the commit made them, each state an
    -- `entity_state_record`, so no state crosses the queue as JSON.
    events pending_change_event[] compression lz4 not null,
    constraint pk_pending_side_effects primary key (id),
    constraint chk_pse_base_ordinal_non_negative check (base_ordinal >= 0),
    constraint chk_pse_attempt_count_non_negative check (attempt_count >= 0),
    constraint chk_pse_claim_coherent check (num_nonnulls (claimed_at, lease_until, claimed_by) in (0, 3)),
    constraint chk_pse_exhausted_state check (exhausted_at is null or num_nonnulls (claimed_at, lease_until, claimed_by) = 0),
    constraint chk_pse_last_error_length check (last_error is null or char_length(last_error) <= 2048),
    constraint chk_pse_events_present check (cardinality(events) > 0),
    constraint uq_pse_write_identity unique (write_id)
)
with (
    vacuum_truncate = false,
    fillfactor = 50,
    autovacuum_vacuum_scale_factor = 0,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

-- The claim. Leased rows are filtered after the scan, which keeps the lease
-- columns out of every index, so a claim stays a heap-only update.
create index idx_pse_claim on pending_side_effects (next_attempt_at)
where
    exhausted_at is null;

-- The reap of dead-lettered jobs.
create index idx_pse_exhausted_at on pending_side_effects (exhausted_at)
where
    exhausted_at is not null;

