-- ============================================================================
-- 0021: Scope history
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.18, 4.19, 5.2.21
--
-- The recorded Scopes of each temporal entity, in the shape of attribute
-- history: the `recorded_at` write clock, the write identity, the four
-- semantic time axes and the tombstone. A row opens a validity interval that
-- runs until the entity's next event on the same axis (clause 4.18), so the
-- table holds one event per change of Scope: a write restating the Scope
-- already in force adds none. It is a plain table in every branch.
--
-- A row carries no instance identity. Clause 4.5.6 gives the temporal Scope
-- only the four non-reified Temporal Properties, so a Scope instance has no
-- `instanceId` to keep, and the write identity is what names a row.
--
-- Codes:
--
--   value_kind: 0 tombstone, 1 scope-path
--
-- A Scope is not an attribute: clause 4.18 gives it no dataset dimension, no
-- attribute family and no catalog entry, so there is no attribute kind and
-- its value kinds are its own. A Scope deletion is a row with `deleted_at` and
-- no path, never an invented sentinel path.
--
-- A recorded Scope is held twice: `scope_path`, the `ltree` label path every
-- clause 4.19 `scopeQ` is matched against, and `verbatim_scope_path`, the
-- Scope as it was written, which is what a Temporal Evolution reads back.
-- Clause 4.18 makes the leading `/` optional and the label path forgets it,
-- while clause 4.6.4 has the broker "return the original content".
--
-- Each semantic axis has a BRIN summary and no B-tree: the entity-led B-trees
-- answer entity-scoped reads, and a broad `observedAt`, `createdAt`,
-- `modifiedAt` or `deletedAt` window that names no entity reads the summary.
-- Each summary covers 32 pages and summarises a range as soon as appends move
-- past it, as attribute history's do, and the table takes an insert-driven
-- vacuum once a twentieth of its rows are new.
-- ============================================================================
create table scope_history (
    recorded_at timestamptz not null,
    temporal_entity_key bigint not null,
    write_id uuid not null,
    write_ordinal integer not null,
    value_kind smallint not null,
    observed_at timestamptz,
    created_at timestamptz,
    modified_at timestamptz,
    deleted_at timestamptz,
    scope_path ltree compression lz4,
    verbatim_scope_path text compression lz4 collate "C",
    constraint uq_scope_history_write_identity unique (recorded_at, write_id, write_ordinal),
    constraint chk_scope_history_write_ordinal check (write_ordinal >= 0),
    constraint chk_scope_history_value_presence check ((value_kind = 0 and deleted_at is not null and scope_path is null and verbatim_scope_path is null) or (value_kind = 1 and scope_path is not null and verbatim_scope_path is not null))
)
with (
    autovacuum_vacuum_insert_scale_factor = 0.05
);

create index idx_scope_history_entity_recorded_at on scope_history (temporal_entity_key, recorded_at desc);

create index idx_scope_history_entity_observed_at on scope_history (temporal_entity_key, observed_at desc)
where
    observed_at is not null;

-- `scopeQ` matching over recorded Scopes (clause 4.19).
create index idx_scope_history_scope_path on scope_history using gist (scope_path)
where
    scope_path is not null;

create index idx_scope_history_observed_at_brin on scope_history using brin (observed_at timestamptz_minmax_multi_ops) with (pages_per_range = 32, autosummarize = on);

create index idx_scope_history_created_at_brin on scope_history using brin (created_at timestamptz_minmax_multi_ops) with (pages_per_range = 32, autosummarize = on);

create index idx_scope_history_modified_at_brin on scope_history using brin (modified_at timestamptz_minmax_multi_ops) with (pages_per_range = 32, autosummarize = on);

create index idx_scope_history_deleted_at_brin on scope_history using brin (deleted_at timestamptz_minmax_multi_ops) with (pages_per_range = 32, autosummarize = on);

