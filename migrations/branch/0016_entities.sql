-- ============================================================================
-- 0016: Entities
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.5.5, 4.5.6, 4.7.1, 4.8, 4.10, 4.17,
--   4.18, 4.22, 5.2.23, 5.5.9, 5.6.6, 5.6.16
--
-- The Entities both APIs serve: the identity row, the search row and one
-- membership row per type the Entity carries, sharing `entity_key`. The
-- attribute instances live in `current_attributes`. The split follows the
-- write pattern, not the data model: an attribute write stamps only the
-- identity row's `write_count` and `modified_at`, which no index covers, so
-- it stays a heap-only update while its page has room; almost no write
-- changes the Entity's types, scopes or geometries, and its identity never
-- changes.
--
-- One identity row serves the Entity's current state and its Temporal
-- Evolution (clause 4.5.6), each of which may exist without the other: a
-- core deletion (clause 5.6.6) keeps the Temporal Evolution, a Temporal
-- Evolution deletion (clause 5.6.16) keeps the current state, and the
-- Temporal API creates an Evolution without one. The row holds the
-- identifier once for both and goes with the last of the two. Only an Entity
-- with a current state has a search row; its membership rows say which of
-- the two each type belongs to.
-- ============================================================================
-- Identity and lifecycle. The fillfactor leaves the room on each page that
-- keeps the `write_count` and `modified_at` update of every core write and the
-- `evolution_modified_at` update of every recording write heap-only.
create table entities (
    entity_key bigint generated always as identity (cache 32),
    -- How many writes of the current state the row has taken. Every core
    -- write, deletion and re-creation moves it, and with `entity_key` it is
    -- the compare-and-set token an optimistic overwrite states: two writes
    -- stamping the same `modified_at` still leave different counts, and an
    -- Entity re-created after a deletion that removed the row gets a new key.
    write_count bigint not null default 1,
    -- Clause 4.8 `createdAt` of the current state, NULL while the Entity has
    -- none. Every core read reaches an Entity through a row whose current
    -- state exists.
    created_at timestamptz,
    -- Clause 4.8 `modifiedAt`, stamped by every write of the Entity.
    modified_at timestamptz,
    expires_at timestamptz,
    -- The earliest `expiresAt` any attribute instance of the Entity carries,
    -- NULL when none expires. The attribute-expiry sweep ranges over it
    -- instead of walking every instance, then re-reads each candidate's
    -- instances and decides per instance.
    min_attribute_expires_at timestamptz,
    -- The Temporal Evolution's clause 4.8 `createdAt` and `modifiedAt`,
    -- stamped by the writes recording its history, NULL while the Entity has
    -- none.
    evolution_created_at timestamptz,
    evolution_modified_at timestamptz,
    -- The `entity_type_names` keys the Temporal Evolution is recorded under.
    -- Clauses 5.6.11.4 and 5.6.12.4 add the types a write states that it
    -- lacks and no operation removes one, so the list only grows; a core
    -- write that changes the current types leaves it alone.
    evolution_type_name_keys integer[] compression lz4,
    id text compression lz4 collate "C" not null,
    constraint pk_entities primary key (entity_key),
    constraint uq_entities_id unique (id),
    constraint chk_entities_id_length check (octet_length(id) <= 1024),
    constraint chk_entities_write_count check (write_count > 0),
    constraint chk_entities_current_state check ((created_at is null) = (modified_at is null) and (created_at is not null or (expires_at is null and min_attribute_expires_at is null))),
    constraint chk_entities_evolution check ((evolution_created_at is null) = (evolution_modified_at is null) and (evolution_created_at is null) = (evolution_type_name_keys is null) and (evolution_type_name_keys is null or cardinality(evolution_type_name_keys) > 0)),
    constraint chk_entities_held check (created_at is not null or evolution_created_at is not null)
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

create index idx_entities_expires_at on entities (expires_at)
where
    expires_at is not null;

create index idx_entities_min_attribute_expires_at on entities (min_attribute_expires_at)
where
    min_attribute_expires_at is not null;

create index idx_entities_id_trgm on entities using gin (id gin_trgm_ops);

-- The query axes: types (clause 4.17), the names of the attributes the
-- Entity carries (clause 5.2.23 `attrs`), scopes (clause 4.18) and the
-- hoisted default `location` instance (clauses 4.7.1 and 4.10); every other
-- `GeoProperty` is found through its `current_attributes` rows, whose
-- geometry index is keyed by attribute name. Every index that finds
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
    -- instances carry in `current_attributes`.
    location_expires_at timestamptz,
    -- `entity_type_names` keys.
    entity_type_name_keys integer[] compression lz4 not null,
    -- `attribute_names` keys.
    attribute_name_keys integer[] compression lz4 not null,
    -- One element per attribute instance the Entity carries (clause 4.5.5:
    -- a `datasetId` instance is its own element): its `attribute_names` key,
    -- and at the same position its attribute family code (the
    -- `current_attributes.attribute_kind` codes). Sorted by key and code,
    -- so a write that lists the same instances in another order does not
    -- rewrite the row. Not indexed: nothing searches them. They are what the
    -- discovery census counts, and the row read back when an update or a
    -- delete takes an Entity's instances out of the census.
    attribute_instance_name_keys integer[] compression lz4 not null,
    attribute_instance_kinds smallint[] compression lz4 not null,
    -- NULL for an Entity without a Scope, never an empty array: no `scopeQ`
    -- matches such an Entity (clause 4.19), so it costs the Scope index
    -- nothing.
    scope_paths ltree[] compression lz4,
    -- The same Scopes as written, in the same order: the label path forgets
    -- the optional leading `/` of clause 4.18, and this is what reads back.
    verbatim_scope_paths text[] compression lz4 collate "C",
    location geometry compression lz4,
    constraint pk_entity_search primary key (entity_key),
    constraint fk_entity_search_entity foreign key (entity_key) references entities (entity_key) on delete cascade,
    constraint chk_entity_search_entity_type_name_keys_present check (cardinality(entity_type_name_keys) > 0),
    constraint chk_entity_search_current_attributes_aligned check (cardinality(attribute_instance_name_keys) = cardinality(attribute_instance_kinds)),
    constraint chk_entity_search_attribute_instance_kinds check (attribute_instance_kinds <@ '{1,2,3,4,5,6,7,8}'::smallint[]),
    constraint chk_entity_search_scope_spelling check ((scope_paths is null) = (verbatim_scope_paths is null) and (scope_paths is null or (cardinality(scope_paths) > 0 and cardinality(scope_paths) = cardinality(verbatim_scope_paths)))),
    constraint chk_entity_search_location_valid check (location is null or st_isvalid(location)),
    constraint chk_entity_search_location_srid check (location is null or st_srid(location) = 4326),
    constraint chk_entity_search_location_zm check (location is null or st_zmflag(location) in (0, 2))
)
with (
    vacuum_truncate = false,
    autovacuum_vacuum_scale_factor = 0,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

create index idx_entity_search_entity_type_name_keys on entity_search using gin (entity_type_name_keys);

create index idx_entity_search_attribute_name_keys on entity_search using gin (attribute_name_keys);

-- A `scopeQ` test is strict, so it implies `scope_paths is not null` and
-- reaches this partial index; an Entity without a Scope is not in it.
create index idx_entity_search_scope_paths on entity_search using gist (scope_paths)
where
    scope_paths is not null;

create index idx_entity_search_location on entity_search using gist (location)
where
    location is not null;

-- `near` measures on the spheroid (clause 4.10), so its distance is judged on
-- the `geography` projection. The index keys the projection itself rather than
-- a stored copy of it: a row holds its geometry once, and a recheck projects
-- the one candidate it judges.
create index idx_entity_search_location_geography on entity_search using gist ((geom_to_geog(location)))
where
    location is not null;

-- The spatial index catch-up: a range scan over recently written rows.
create index idx_entity_search_written_xid on entity_search (written_xid);

alter table entity_search
    alter column entity_type_name_keys set statistics 1000;

-- One row per type an Entity carries in its current state or its Temporal
-- Evolution (clause 4.17 selects on membership), keyed by type and then
-- identifier. A type selection pages by walking one range of this key in the
-- clause 5.5.9 `id` order, reading exactly the rows it skips and keeps, and
-- keeps the rows of the side it reads: a core read those of the current
-- state, a temporal read those of the Evolution. The two hold the same types
-- unless a core write changed the current ones, so one row serves both. The
-- GIN over `entity_search.entity_type_name_keys` cannot give that order, and
-- a walk of `uq_entities_id` filtered by type reads every Entity whose
-- identifier sorts ahead of the type's first. The key carries `entity_key`
-- and both sides so the walk never visits the heap, and the insert-driven
-- vacuum keeps the visibility map current enough for that to hold.
create table entity_type_members (
    entity_key bigint not null,
    entity_type_name_key integer not null,
    -- Whether the current state carries the type.
    is_current_member boolean not null,
    -- Whether the Temporal Evolution is recorded under the type.
    is_evolution_member boolean not null,
    id text compression lz4 collate "C" not null,
    constraint pk_entity_type_members primary key (entity_type_name_key, id) include (entity_key, is_current_member, is_evolution_member),
    constraint fk_entity_type_members_entity foreign key (entity_key) references entities (entity_key) on delete cascade,
    constraint chk_entity_type_members_side check (is_current_member or is_evolution_member),
    constraint chk_entity_type_members_id_length check (octet_length(id) <= 1024)
)
with (
    vacuum_truncate = false,
    autovacuum_vacuum_insert_scale_factor = 0,
    autovacuum_vacuum_insert_threshold = 500,
    autovacuum_vacuum_scale_factor = 0,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

-- The cascade from `entities` and a type change find an Entity's rows by key.
create index idx_entity_type_members_entity_key on entity_type_members (entity_key);
