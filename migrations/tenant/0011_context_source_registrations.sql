-- ============================================================================
-- 0011: Context source registrations and their delivery state
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.3.6, 4.5.5, 4.19, 4.20, 5.2.9, 5.9,
--   5.10
--
-- What each remote context source declares it can provide. Distributed
-- operations consult these registrations to decide which context sources a
-- request must reach beyond the entities held locally.
--
-- Codes:
--
--   mode: 1 inclusive, 2 exclusive, 3 redirect, 4 auxiliary
--   operations: 1 createEntity, 2 updateEntity, 3 appendAttrs, 4 updateAttrs,
--     5 deleteAttrs, 6 deleteEntity, 7 createBatch, 8 upsertBatch,
--     9 updateBatch, 10 deleteBatch, 11 upsertTemporal,
--     12 appendAttrsTemporal, 13 deleteAttrsTemporal,
--     14 updateAttrInstanceTemporal, 15 deleteAttrInstanceTemporal,
--     16 deleteTemporal, 17 mergeEntity, 18 replaceEntity, 19 replaceAttrs,
--     20 mergeBatch, 21 purgeEntity, 22 retrieveEntity, 23 queryEntity,
--     24 queryBatch, 25 retrieveTemporal, 26 queryTemporal,
--     27 retrieveEntityTypes, 28 retrieveEntityTypeDetails,
--     29 retrieveEntityTypeInfo, 30 retrieveAttrTypes,
--     31 retrieveAttrTypeDetails, 32 retrieveAttrTypeInfo,
--     33 createSubscription, 34 updateSubscription, 35 retrieveSubscription,
--     36 querySubscription, 37 deleteSubscription, 38 retrieveEntityMap,
--     39 updateEntityMap, 40 deleteEntityMap, 41 createEntityMapQueryEntity,
--     42 createEntityMapQueryTemporal, 43 retrieveContextSourceIdentity,
--     44 federationOps, 45 associationOps, 46 updateOps, 47 retrieveOps,
--     48 redirectionOps
--   registration_delivery_state.status: 1 ok, 2 failed
--
-- The five operation groups of Table 4.20-2 are stored as declared, never
-- expanded into the operations they stand for: expanding them would freeze one
-- reading of that table into every stored row.
--
-- There is no `type` column. Every registration's type is
-- `ContextSourceRegistration` (clause 5.2.9), so the broker renders the
-- constant rather than storing it on every row.
-- ============================================================================
create table context_source_registrations (
    created_at timestamptz not null default now(),
    modified_at timestamptz not null default now(),
    -- NULL means the registration never expires.
    expires_at timestamptz,
    mode smallint not null,
    id text compression lz4 collate "C" not null,
    registration_name text compression lz4,
    description text compression lz4,
    -- The revision of the broker's rendering that produced the
    -- `entity_id_pattern_form` of this registration's information rows, NULL
    -- when the registration declares no `idPattern`. A broker renders again,
    -- when it opens the tenant, every row an older revision produced.
    entity_id_pattern_forms_revision smallint,
    -- The tenant to address at the context source; NULL means its default
    -- tenant.
    tenant text compression lz4,
    -- The two clause 5.2.11 intervals, each NULL when its member is absent.
    observation_interval_start timestamptz,
    observation_interval_end timestamptz,
    management_interval_start timestamptz,
    management_interval_end timestamptz,
    -- Each geometry as written (clause 4.7.1), beside its `ST_MakeValid` form
    -- when the written one is invalid (clause 4.7.2 admits it, IETF RFC 7946
    -- section 3.1.6 sets no simplicity rule on a ring), NULL otherwise. A
    -- geoquery and the spatial indexes judge `coalesce(<x>_valid, <x>)`.
    location geometry compression lz4,
    location_valid geometry compression lz4,
    observation_space geometry compression lz4,
    observation_space_valid geometry compression lz4,
    operation_space geometry compression lz4,
    operation_space_valid geometry compression lz4,
    endpoint text compression lz4 not null,
    -- The media type the broker sends as `Accept` to this context source.
    accept text compression lz4,
    -- NULL is the absent member, which reads as the clause 5.2.9 default,
    -- `federationOps`.
    operations smallint[] compression lz4,
    -- Stored in the label-path encoding of `entity_search.scope_paths`, so a
    -- clause 4.19 `scopeQ` against registrations compiles to the same `lquery`
    -- match that selects entities.
    scope_paths ltree[] compression lz4,
    -- The same Scopes as written, in the same order: the label path forgets
    -- the optional leading `/` of clause 4.18, and this is what reads back.
    verbatim_scope_paths text[] compression lz4 collate "C",
    refresh_rate text compression lz4,
    -- Whether the registration declares `management` at all: an empty
    -- `management` object reads back as written, with every member below
    -- NULL.
    is_management_declared boolean not null,
    -- Table 5.2.34-1, each NULL when its member is absent. The duration is
    -- held in the ISO 8601 text the broker writes; the two periods are
    -- milliseconds.
    management_cache_duration text compression lz4,
    management_cooldown bigint,
    management_local_only boolean,
    management_timeout bigint,
    -- `contextSourceInfo` as two parallel arrays in written order, both NULL
    -- when the member is absent and both empty when it is an empty list.
    context_source_info_keys text[] compression lz4,
    context_source_info_values text[] compression lz4,
    -- Members outside Table 5.2.9-1, kept for the representation.
    additional_properties jsonb compression lz4,
    -- What a clause 4.9 `csf` filter reads: every Table 5.2.9-1 member the
    -- registration declares, keyed by the IRI the core context expands its
    -- name to, then the members outside the table under their expanded names.
    -- Written whole on every create and update.
    context_source_properties jsonb compression lz4 not null,
    -- Table 5.2.9-1 `datasetId`: each element a URI, or `@none` for the
    -- default instance (clause 4.5.5). Unindexed: nothing preselects
    -- registrations by dataset.
    dataset_id text[] compression lz4,
    context_source_alias text compression lz4,
    constraint pk_context_source_registrations primary key (id),
    constraint chk_context_source_registrations_id_length check (octet_length(id) <= 1024),
    constraint chk_csr_mode check (mode between 1 and 4),
    constraint chk_csr_observation_interval_end_after_start check (observation_interval_end is null or (observation_interval_start is not null and observation_interval_end > observation_interval_start)),
    constraint chk_csr_management_interval_end_after_start check (management_interval_end is null or (management_interval_start is not null and management_interval_end > management_interval_start)),
    constraint chk_csr_management_presence check (is_management_declared or num_nonnulls (management_cache_duration, management_cooldown, management_local_only, management_timeout) = 0),
    constraint chk_csr_management_cooldown_positive check (management_cooldown > 0),
    constraint chk_csr_management_timeout_positive check (management_timeout > 0),
    constraint chk_csr_context_source_info_pairs check ((context_source_info_keys is null) = (context_source_info_values is null) and cardinality(context_source_info_keys) = cardinality(context_source_info_values) and array_position(context_source_info_keys, null) is null and array_position(context_source_info_values, null) is null),
    constraint chk_csr_operations check (operations is null or (cardinality(operations) > 0 and array_position(operations, null) is null and 1 <= all (operations) and 48 >= all (operations))),
    constraint chk_csr_location_valid check (location_valid is null or (location is not null and st_isvalid(location_valid) and not st_isvalid(location))),
    constraint chk_csr_location_srid check (location is null or st_srid(location) = 4326),
    constraint chk_csr_location_zm check (location is null or st_zmflag(location) in (0, 2)),
    constraint chk_csr_observation_space_valid check (observation_space_valid is null or (observation_space is not null and st_isvalid(observation_space_valid) and not st_isvalid(observation_space))),
    constraint chk_csr_observation_space_srid check (observation_space is null or st_srid(observation_space) = 4326),
    constraint chk_csr_observation_space_zm check (observation_space is null or st_zmflag(observation_space) in (0, 2)),
    constraint chk_csr_operation_space_valid check (operation_space_valid is null or (operation_space is not null and st_isvalid(operation_space_valid) and not st_isvalid(operation_space))),
    constraint chk_csr_operation_space_srid check (operation_space is null or st_srid(operation_space) = 4326),
    constraint chk_csr_operation_space_zm check (operation_space is null or st_zmflag(operation_space) in (0, 2)),
    constraint chk_csr_scope_spelling check ((scope_paths is null) = (verbatim_scope_paths is null) and (scope_paths is null or cardinality(scope_paths) = cardinality(verbatim_scope_paths)))
);

-- A geoquery keeps the registrations whose geometry meets it and those that
-- declare none, so discovery reads the two as separate parts: the spatial
-- indexes hold the declared geometries, and the identifier indexes beside them
-- hold the registrations without one, in the identifier order a page is cut in.
create index idx_csr_location on context_source_registrations using gist (coalesce(location_valid, location))
where
    location is not null;

create index idx_csr_without_location on context_source_registrations (id)
where
    location is null;

create index idx_csr_observation_space on context_source_registrations using gist (coalesce(observation_space_valid, observation_space))
where
    observation_space is not null;

create index idx_csr_without_observation_space on context_source_registrations (id)
where
    observation_space is null;

create index idx_csr_operation_space on context_source_registrations using gist (coalesce(operation_space_valid, operation_space))
where
    operation_space is not null;

create index idx_csr_without_operation_space on context_source_registrations (id)
where
    operation_space is null;

create index idx_csr_scope_paths on context_source_registrations using gist (scope_paths);

create index idx_csr_expires_at on context_source_registrations (expires_at)
where
    expires_at is not null;

-- The forwarding statistics and health of clause 5.2.9. Every forwarded
-- request moves them, so they live apart from the registration's wide row.
-- The row is created in the registration's own create transaction. Readers
-- test `status` through a primary-key join on registrations the routing
-- projection has already narrowed, so a `status` index would have no scan to
-- speed up.
create table registration_delivery_state (
    last_success timestamptz,
    last_failure timestamptz,
    times_sent integer not null default 0,
    times_failed integer not null default 0,
    status smallint not null,
    registration_id text compression lz4 collate "C" not null,
    constraint pk_registration_delivery_state primary key (registration_id),
    constraint fk_registration_delivery_state_registration foreign key (registration_id) references context_source_registrations (id) on delete cascade,
    constraint chk_registration_delivery_state_registration_id_length check (octet_length(registration_id) <= 1024),
    -- The counters floor at zero. A counter that has not moved is rendered as
    -- an absent member, but the column holds a value before it first advances.
    constraint chk_registration_delivery_state_times_sent_non_negative check (times_sent >= 0),
    constraint chk_registration_delivery_state_times_failed_non_negative check (times_failed >= 0),
    constraint chk_registration_delivery_state_status check (status between 1 and 2)
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
