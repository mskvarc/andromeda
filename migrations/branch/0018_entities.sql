-- ============================================================================
-- 0018: Entities
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.5.5, 4.7.1, 4.8, 4.10, 4.17, 4.18,
--   4.22, 5.2.23, 5.5.9
--
-- Current Entity state, as three rows in three relations sharing
-- `entity_key`, and one membership row per type the Entity carries. The split
-- follows the write pattern, not the data model: every attribute write
-- rewrites the payload, almost none changes the Entity's types, scopes or
-- geometries, and its identity never changes. The payload therefore lives
-- apart from every indexed column, so a payload write stays a heap-only update
-- and leaves the type, scope and geometry index entries untouched.
-- ============================================================================
-- Identity and lifecycle.
create table entities (
    entity_key bigint generated always as identity (cache 32),
    created_at timestamptz not null default now(),
    expires_at timestamptz,
    -- The earliest `expiresAt` any attribute instance of the payload carries,
    -- NULL when none expires. The attribute-expiry sweep ranges over it
    -- instead of walking every payload, then re-reads each candidate's
    -- payload and decides per instance.
    min_attribute_expires_at timestamptz,
    id text compression lz4 collate "C" not null,
    constraint pk_entities primary key (entity_key),
    constraint uq_entities_id unique (id),
    constraint chk_entities_id_length check (octet_length(id) <= 1024)
)
with (
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

create index idx_entities_expires_at on entities (expires_at)
where
    expires_at is not null;

create index idx_entities_min_attribute_expires_at on entities (min_attribute_expires_at)
where
    min_attribute_expires_at is not null;

create index idx_entities_id_trgm on entities using gin (id gin_trgm_ops);

-- The payload and its version token. It has no index but its primary key,
-- and must never gain one: every payload write is an update of this row, and
-- any index would stop those updates being heap-only.
create table entity_documents (
    entity_key bigint not null,
    -- Clause 4.8 `modifiedAt`, kept here because it moves with the payload:
    -- the same statement stamps both, and it is the compare-and-set token an
    -- optimistic overwrite states.
    modified_at timestamptz not null default now(),
    payload jsonb compression lz4 not null,
    constraint pk_entity_documents primary key (entity_key),
    constraint fk_entity_documents_entity foreign key (entity_key) references entities (entity_key) on delete cascade
)
with (
    fillfactor = 70,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

-- The query axes: types (clause 4.17), the names of the attributes the
-- payload carries (clause 5.2.23 `attrs`), scopes (clause 4.18) and the three
-- hoisted geometries (clauses 4.7.1 and 4.10). Every index that finds
-- entities is here, and a row is rewritten only when one of these columns
-- changes. It keeps the default fillfactor: its updates are rare, and they
-- rewrite indexed columns anyway.
create table entity_search (
    entity_key bigint not null,
    -- The `xid8` of the transaction that last wrote the row: the insert takes
    -- it from the default and every update sets it. A broker that keeps an
    -- in-memory spatial index reads the rows written at or above the `xmin`
    -- of its previous read's snapshot to catch up with every change,
    -- whichever process committed it; an `xid8` never wraps, and a
    -- transaction below a snapshot's `xmin` had finished before the snapshot
    -- was taken.
    written_xid xid8 not null default pg_current_xact_id(),
    -- The clause 4.22 `expiresAt` of the hoisted default `location` instance,
    -- NULL when it carries none. A geo-query reads the hoisted instance only
    -- while this is NULL or ahead of NOW(): clause 4.22 invalidates that one
    -- instance at its own expiry, whatever the Entity's other `location`
    -- instances carry in `current_attribute_values`.
    location_expires_at timestamptz,
    -- `entity_type_names` keys.
    entity_type_name_keys integer[] compression lz4 not null,
    -- `attribute_names` keys.
    attribute_name_keys integer[] compression lz4 not null,
    -- One element per attribute instance the payload carries (clause 4.5.5:
    -- a `datasetId` instance is its own element): its `attribute_names` key,
    -- and at the same position its attribute family code (the
    -- `current_attribute_values.attribute_kind` codes). Sorted by key and code,
    -- so a write that lists the same instances in another order does not
    -- rewrite the row. Not indexed: nothing searches them. They are what the
    -- discovery census counts, and the row read back when an update or a
    -- delete takes an Entity's instances out of the census.
    attribute_instance_name_keys integer[] compression lz4 not null,
    attribute_instance_kinds smallint[] compression lz4 not null,
    scope_paths ltree[] compression lz4,
    location geometry compression lz4,
    location_geography geography compression lz4 generated always as (geom_to_geog(location)) stored,
    observation_space geometry compression lz4,
    operation_space geometry compression lz4,
    constraint pk_entity_search primary key (entity_key),
    constraint fk_entity_search_entity foreign key (entity_key) references entities (entity_key) on delete cascade,
    constraint chk_entity_search_entity_type_name_keys_present check (cardinality(entity_type_name_keys) > 0),
    constraint chk_entity_search_attribute_instances_aligned check (cardinality(attribute_instance_name_keys) = cardinality(attribute_instance_kinds)),
    constraint chk_entity_search_attribute_instance_kinds check (attribute_instance_kinds <@ '{1,2,3,4,5,6,7,8}'::smallint[]),
    constraint chk_entity_search_location_valid check (location is null or st_isvalid(location)),
    constraint chk_entity_search_location_srid check (location is null or st_srid(location) = 4326),
    constraint chk_entity_search_location_zm check (location is null or st_zmflag(location) in (0, 2)),
    constraint chk_entity_search_observation_space_valid check (observation_space is null or st_isvalid(observation_space)),
    constraint chk_entity_search_observation_space_srid check (observation_space is null or st_srid(observation_space) = 4326),
    constraint chk_entity_search_observation_space_zm check (observation_space is null or st_zmflag(observation_space) in (0, 2)),
    constraint chk_entity_search_operation_space_valid check (operation_space is null or st_isvalid(operation_space)),
    constraint chk_entity_search_operation_space_srid check (operation_space is null or st_srid(operation_space) = 4326),
    constraint chk_entity_search_operation_space_zm check (operation_space is null or st_zmflag(operation_space) in (0, 2))
)
with (
    autovacuum_vacuum_scale_factor = 0.02,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

create index idx_entity_search_entity_type_name_keys on entity_search using gin (entity_type_name_keys);

create index idx_entity_search_attribute_name_keys on entity_search using gin (attribute_name_keys);

create index idx_entity_search_scope_paths on entity_search using gist (scope_paths);

create index idx_entity_search_location on entity_search using gist (location)
where
    location is not null;

create index idx_entity_search_location_geography on entity_search using gist (location_geography)
where
    location_geography is not null;

create index idx_entity_search_observation_space on entity_search using gist (observation_space)
where
    observation_space is not null;

create index idx_entity_search_operation_space on entity_search using gist (operation_space)
where
    operation_space is not null;

-- The spatial index catch-up: a range scan over recently written rows.
create index idx_entity_search_written_xid on entity_search (written_xid);

alter table entity_search
    alter column entity_type_name_keys set statistics 1000;

-- One row per type an Entity carries (clause 4.17 selects on membership),
-- keyed by type and then identifier. A type selection pages by walking one
-- range of this key in the clause 5.5.9 `id` order, reading exactly the rows it
-- skips and keeps. The GIN over `entity_search.entity_type_name_keys` cannot
-- give that order, and a walk of `uq_entities_id` filtered by type reads every
-- Entity whose identifier sorts ahead of the type's first. The key carries
-- `entity_key` so the walk never visits the heap, and the insert-driven vacuum
-- keeps the visibility map current enough for that to hold.
create table entity_type_members (
    entity_key bigint not null,
    entity_type_name_key integer not null,
    id text compression lz4 collate "C" not null,
    constraint pk_entity_type_members primary key (entity_type_name_key, id) include (entity_key),
    constraint fk_entity_type_members_entity foreign key (entity_key) references entities (entity_key) on delete cascade
)
with (
    autovacuum_vacuum_insert_scale_factor = 0.01,
    autovacuum_vacuum_insert_threshold = 500,
    autovacuum_vacuum_scale_factor = 0.02,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

-- The cascade from `entities` and a type change find an Entity's rows by key.
create index idx_entity_type_members_entity_key on entity_type_members (entity_key);

