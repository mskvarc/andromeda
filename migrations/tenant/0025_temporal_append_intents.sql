-- ============================================================================
-- 0025: Temporal append intents
-- ============================================================================
-- Temporal history appends the broker accepted together with current Entity
-- state but has not yet applied to the temporal catalog and history. A
-- background worker leases them in `intent_key` order and applies them in
-- SQL: an intent's `entries` are `temporal_entry` values that hold exactly the
-- columns their history rows will hold, so applying one moves them into
-- history and deletes the intent in one transaction.
--
-- There is no `exhausted_at`. An intent is history the broker has already
-- acknowledged, so it is never discarded: an intent that keeps failing stays
-- claimable at a bounded retry cadence and is reported once its attempts pass
-- an operator threshold.
--
-- The queue exists only in the live branch. Deferral buys live ingest
-- throughput; a Snapshot branch records its history in the write's own
-- transaction, and no worker drains a branch.
-- ============================================================================
create table temporal_append_intents (
    -- No sequence cache. The intents of one Entity take their keys in the
    -- order its writes commit, and the latest intent is read by key; a block
    -- of keys cached per session would hand a later write a smaller key.
    intent_key bigint generated always as identity,
    created_at timestamptz not null default now(),
    next_attempt_at timestamptz not null default now(),
    -- The first ordinal the intent's entries occupy in the write's numbering.
    base_ordinal integer not null,
    attempt_count integer not null default 0,
    -- The atomic write the intent belongs to, shared by every intent of that
    -- write, so it is not the intent's identity, and with `entity_id` it is
    -- not either: the identity of an intent is the first ordinal it occupies
    -- in the write's one numbering, hence the unique triple, which also makes
    -- a commit replayed under its write identity enqueue nothing twice.
    write_id uuid not null,
    claimed_at timestamptz,
    lease_until timestamptz,
    claimed_by uuid,
    -- The write's ingestion instant, every entry's `recorded_at`.
    recorded_at timestamptz not null,
    entity_id text compression lz4 collate "C" not null,
    -- `entity_type_names` keys of the types the Temporal Evolution is recorded
    -- under (clause 4.5.6), empty for an intent that only ends instances.
    entity_type_name_keys integer[] compression lz4 not null,
    entries temporal_entry[] compression lz4 not null,
    -- The codes of the application statements' needs the entries meet, as
    -- the `temporal_intent` the commit bound carries them: an application
    -- reads this instead of `entries` to skip the statements none of its
    -- intents reaches.
    entry_needs smallint[] compression lz4 not null,
    last_error text compression lz4,
    constraint pk_temporal_append_intents primary key (intent_key),
    constraint uq_temporal_append_intents_write_identity unique (write_id, entity_id, base_ordinal),
    constraint chk_temporal_append_intents_attempt_count_non_negative check (attempt_count >= 0),
    constraint chk_temporal_append_intents_base_ordinal_non_negative check (base_ordinal >= 0),
    constraint chk_temporal_append_intents_claim_coherent check (num_nonnulls (claimed_at, lease_until, claimed_by) in (0, 3)),
    constraint chk_temporal_append_intents_last_error_length check (last_error is null or char_length(last_error) <= 2048),
    constraint chk_temporal_append_intents_entity_id_length check (octet_length(entity_id) <= 1024)
)
with (
    vacuum_truncate = false,
    fillfactor = 70,
    autovacuum_vacuum_scale_factor = 0,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

-- The claim. `intent_key` is its second key because the claim orders by it,
-- so older work is never starved behind newer.
create index idx_temporal_append_intents_claim on temporal_append_intents (next_attempt_at, intent_key);

-- The claim scoped to one Entity, in key order, and the probe for whether an
-- Entity holds a pending intent.
create index idx_temporal_append_intents_entity on temporal_append_intents (entity_id, intent_key);

-- Every entry of every intent beside the write identity its history row will
-- carry: `write_ordinal` is the intent's `base_ordinal` plus the entry's
-- position counted from 0, as the application numbers it. A temporal read of
-- the live branch unions these entries with the recorded history, so an entry
-- answers the same whether or not it has been applied yet.
create view temporal_append_entries as
select
    i.intent_key,
    i.entity_id,
    i.recorded_at,
    i.write_id,
    (i.base_ordinal + e.ordinality - 1)::integer as write_ordinal,
    e.entry_kind,
    e.attribute_name_key,
    e.dataset_id,
    e.attribute_kind,
    e.value_kind,
    e.observed_at,
    e.created_at,
    e.modified_at,
    e.deleted_at,
    e.value_date_time,
    e.expires_at,
    e.generated_instance_id,
    e.value_type_key,
    e.value_datatype_key,
    e.unit_code_id,
    e.value_boolean,
    e.verbatim_instance_id,
    e.value_number,
    e.value_string,
    e.value_language,
    e.value_geometry,
    e.value_geometry_members,
    e.value_object,
    e.value_uri,
    e.object_type_keys,
    e.proof_seals,
    e.sub_attributes,
    e.scope_path,
    e.verbatim_scope_path
from
    temporal_append_intents i
    cross join lateral unnest(i.entries)
    with ordinality as e;
