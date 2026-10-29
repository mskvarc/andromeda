-- ============================================================================
-- 0021: Current attributes
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.5.2, 4.5.3, 4.5.5, 4.6.3, 4.8, 4.9,
--   4.10, 4.22, 4.23, 5.6.6
--
-- The current state of every attribute instance, one row per Entity,
-- attribute name and `datasetId` (clause 4.5.5), in one typed column per value
-- shape so a `q` comparison reads a single indexed column. The row is the only
-- copy of the instance: it holds every member an instance carries, and its
-- sub-attributes (clause 4.5.2) as one `sub_attribute[]` in written
-- order. Sub-attributes exist only inside their instance, so they live inside
-- its row, and changing one rewrites that row alone. Core Entity deletion
-- (clause 5.6.6) leaves history standing, so nothing here references the
-- temporal catalog.
--
-- Codes:
--
--   attribute_kind: 1 Property, 2 Relationship, 3 GeoProperty,
--     4 VocabProperty, 5 LanguageProperty, 6 JsonProperty, 7 ListProperty,
--     8 ListRelationship
--   value_kind: 1 number, 2 string, 3 boolean, 4 geometry, 5 object,
--     6 date-time, 7 uri, 8 date, 9 time
--
-- Value kind 0, the tombstone, never appears: a deleted attribute is an absent
-- row, not a marker.
--
-- A value declared as a DateTime, by its datatype or by the `valueType` of
-- its attribute, lands in `value_date_time` (kind 6); `value_datatype_key`
-- names its datatype when the literal carries one. A declared Date (kind 8)
-- lands in `value_string` in its one spelling, a declared Time (kind 9) in
-- `value_string` as `HH:MM:SS.ffffffZ`, whose byte order is its chronological
-- order. A string declared as no time value is a String (kind 2), whatever
-- it resembles.
--
-- `value_object` holds every value that is not one scalar: a structured
-- value, several values, a JSON literal, a LanguageProperty map, several
-- VocabProperty terms, a ListProperty body, and the objects of a
-- Relationship with several objects or of a ListRelationship, in written
-- order. A single Relationship object stays in `value_uri` or
-- `value_long_uri`, where a traversal joins it.
--
-- `sub_eq_keys` holds the clause 4.9 equality keys of every scalar value of
-- the instance's sub-attribute tree, which the broker computes from the
-- elements it writes: one key per value, salted by the value's name path and
-- kind, so a `q` equality on a sub-attribute probes one GIN index. A key
-- selects candidates only, and every candidate is rechecked against the
-- element it names.
--
-- `expires_at` is the instance's clause 4.22 `expiresAt`, NULL when it never
-- expires. Every read excludes a past-due instance with
-- `expires_at IS NULL OR expires_at > NOW()`; the expiry sweep deletes the row
-- later, finding candidates through `entities.min_attribute_expires_at`.
--
-- No foreign key reaches the keys inside `sub_attributes`, and a per-row
-- foreign key probes the referenced index once per row. Statement-level
-- triggers check the references instead, once per statement over its
-- transition table. PostgreSQL admits a transition table only on a trigger
-- with one event, so each check is a pair, one after insert and one after
-- update, sharing one function. A check locks the
-- referenced rows `FOR KEY SHARE`, the lock a foreign key takes, so a
-- concurrent deletion of a referenced Entity waits for the writing
-- transaction or makes the check fail. The attribute name references are
-- checked on the live branch only, where the dictionaries' references of
-- every live relation are declared.
-- ============================================================================
create table current_attributes (
    entity_key bigint not null,
    created_at timestamptz not null,
    modified_at timestamptz not null,
    -- `attribute_names` key.
    attribute_name_key integer not null,
    -- The written position of the attribute among the Entity's attributes and
    -- of the instance among the attribute's datasets, from 0.
    ordinal integer not null,
    attribute_kind smallint not null,
    value_kind smallint not null,
    value_date_time timestamptz,
    observed_at timestamptz,
    expires_at timestamptz,
    -- `entity_type_names` key of the datatype a typed literal declares, beside
    -- the scalar column holding its value.
    value_datatype_key integer,
    -- `entity_type_names` key of the clause 4.5.2 `valueType`.
    value_type_key integer,
    -- `cefact-units` identity of the clause 4.5.2 `unitCode`. The library
    -- assigns identities append-only and never reuses one, and the broker
    -- refuses to decode a value its library does not list.
    unit_code_id smallint,
    value_boolean boolean,
    dataset_id text compression lz4 collate "C",
    -- `NUMERIC`, not `DOUBLE PRECISION`: JSON numbers are stored exactly, and
    -- a comparison beyond 2^53 stays correct.
    value_number numeric compression lz4,
    value_string text compression lz4 collate "C",
    -- The language tag of a language-tagged literal held in `value_string` or
    -- `value_long_text`. It has no length bound: no index keys it, and
    -- BCP 47 sets no length on a tag.
    value_language text compression lz4 collate "C",
    -- The geometry as written: clause 4.7.1 makes the GeoProperty value the
    -- value read back, and clause 4.7.2 admits every RFC 7946 geometry, whose
    -- section 3.1.6 sets no simplicity rule on a ring.
    value_geometry geometry compression lz4,
    -- The `ST_MakeValid` form of an invalid `value_geometry`, NULL when the
    -- written one is valid. Spatial predicates judge
    -- `coalesce(value_geometry_valid, value_geometry)`.
    value_geometry_valid geometry compression lz4,
    -- The rectangle the written geometry covers when its edges are read as
    -- the great-circle arcs `near` measures along (clause 4.10), NULL where
    -- it extends past the planar bounding box by no more than 1e-6 degrees.
    -- Repairing a geometry only adds vertices on its edges, so the reach of
    -- the written form also bounds the repaired one. The geometry index keys
    -- `coalesce(value_geometry_reach, value_geometry_valid, value_geometry)`.
    value_geometry_reach geometry compression lz4,
    -- The members of the GeoJSON geometry beside its type and coordinates:
    -- `bbox` and foreign members (RFC 7946 sections 5 and 6.1).
    value_geometry_members jsonb compression lz4,
    value_object jsonb compression lz4,
    -- A Relationship object no longer than an identifier may be, so every
    -- object naming a local Entity sits here, where a traversal joins it.
    value_uri text compression lz4 collate "C",
    -- A String too long for the B-tree on `value_string`. The two length
    -- checks give every String exactly one column, so an equality reads only
    -- the column its operand would occupy.
    value_long_text text compression lz4 collate "C",
    -- A Relationship object too long for the B-tree on `value_uri`: clause
    -- 4.6.2 sets no length on a URI. The same two-check placement as the
    -- String overflow.
    value_long_uri text compression lz4 collate "C",
    -- `entity_type_names` keys of the clause 4.5.3 `objectType` list, in
    -- written order.
    object_type_keys integer[] compression lz4,
    -- The clause 4.5.2.2 `entityIdSealed` and `entityTypeSealed` members of an
    -- instance, NULL when it carries neither.
    proof_seals jsonb compression lz4,
    -- The instance's sub-attributes, NULL when it has none.
    sub_attributes sub_attribute[] compression lz4,
    -- The equality keys of the sub-attribute tree's scalar values, NULL when
    -- it holds none.
    sub_eq_keys bigint[] compression lz4,
    constraint chk_current_attributes_attribute_kind check (attribute_kind between 1 and 8),
    constraint chk_current_attributes_unit_code_id_positive check (unit_code_id > 0),
    constraint chk_current_attributes_ordinal_non_negative check (ordinal >= 0),
    constraint chk_current_attributes_value_presence check (num_nonnulls(value_number, value_string, value_boolean, value_geometry, value_object, value_date_time, value_uri, value_long_text, value_long_uri) = 1),
    constraint chk_current_attributes_value_kind_column check ((value_kind = 1 and value_number is not null) or (value_kind = 2 and num_nonnulls(value_string, value_long_text) = 1) or (value_kind = 3 and value_boolean is not null) or (value_kind = 4 and value_geometry is not null) or (value_kind = 5 and value_object is not null) or (value_kind = 6 and value_date_time is not null) or (value_kind = 7 and num_nonnulls(value_uri, value_long_uri) = 1) or (value_kind = 8 and value_string is not null) or (value_kind = 9 and value_string is not null)),
    constraint chk_current_attributes_value_datatype_with_scalar check (value_datatype_key is null or value_kind in (1, 2, 3, 6, 8, 9)),
    constraint chk_current_attributes_value_language_with_string check (value_language is null or (value_kind = 2 and value_datatype_key is null)),
    constraint chk_current_attributes_value_geometry_members_with_geometry check (value_geometry_members is null or value_geometry is not null),
    constraint chk_current_attributes_value_geometry_valid check (value_geometry_valid is null or (value_geometry is not null and st_isvalid(value_geometry_valid) and not st_isvalid(value_geometry))),
    constraint chk_current_attributes_value_geometry_srid check (value_geometry is null or st_srid(value_geometry) = 4326),
    constraint chk_current_attributes_value_geometry_zm check (value_geometry is null or st_zmflag(value_geometry) in (0, 2)),
    constraint chk_current_attributes_value_geometry_reach check (value_geometry_reach is null or (value_geometry is not null and st_srid(value_geometry_reach) = 4326 and value_geometry_reach ~ value_geometry)),
    constraint chk_current_attributes_value_string_length check (octet_length(value_string) <= 2048),
    constraint chk_current_attributes_value_long_text_length check (octet_length(value_long_text) > 2048),
    constraint chk_current_attributes_value_uri_length check (octet_length(value_uri) <= 1024),
    constraint chk_current_attributes_value_long_uri_length check (octet_length(value_long_uri) > 1024),
    constraint chk_current_attributes_dataset_id_length check (octet_length(dataset_id) <= 1024),
    -- The empty string is the grain's key for the default instance, so no
    -- `datasetId` may be it.
    constraint chk_current_attributes_dataset_id_not_empty check (dataset_id <> ''),
    constraint chk_current_attributes_sub_eq_keys_not_empty check (cardinality(sub_eq_keys) > 0)
)
with (
    vacuum_truncate = false,
    fillfactor = 80,
    autovacuum_vacuum_scale_factor = 0,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

-- The grain, its default instance keyed as the empty string; the table has no
-- primary key. It is the upsert's conflict target, the range a whole-Entity
-- read scans and the access path of every Entity-correlated `q` probe. A
-- grain compared with `is not distinct from` is joined by testing every pair
-- of rows, so a write's instances matched against its Entity's would cost
-- their product; keyed by `coalesce`, the same join is a hash join or an
-- index probe. Every statement spells the grain as this expression.
create unique index uq_current_attributes_grain on current_attributes (entity_key, attribute_name_key, (coalesce(dataset_id, '')));

-- One partial B-tree per typed column, led by the attribute key, for plans
-- that drive from the attribute side.
create index idx_ca_value_number on current_attributes (attribute_name_key, value_number)
where
    value_number is not null;

create index idx_ca_value_string on current_attributes (attribute_name_key, value_string)
where
    value_string is not null;

-- Unanchored `~=` patterns (clause 4.9). Strings of one attribute share most
-- of their trigrams, so concurrent writers inserting into the posting pages
-- directly queue on the same pages; a pending list takes the inserts instead,
-- and its 64 kB bound keeps the flush a writer pays, and the list a read
-- scans, small.
create index idx_ca_value_string_trgm on current_attributes using gin (attribute_name_key int4_ops, value_string gin_trgm_ops) with (fastupdate = on, gin_pending_list_limit = 64)
where
    value_string is not null;

create index idx_ca_value_boolean on current_attributes (attribute_name_key, value_boolean)
where
    value_boolean is not null;

create index idx_ca_value_date_time on current_attributes (attribute_name_key, value_date_time)
where
    value_date_time is not null;

create index idx_ca_value_uri on current_attributes (attribute_name_key, value_uri)
where
    value_uri is not null;

-- A long String is too long to key a B-tree on, so a `q` comparison over it
-- drives from the attribute key alone and filters the value on the heap. It
-- is not a maintenance target: a value update leaves its key unchanged, so
-- bottom-up index deletion removes the version churn as it happens.
create index idx_ca_value_long_text on current_attributes (attribute_name_key)
where
    value_long_text is not null;

-- Patterns over long Strings, and their equality, which `gin_trgm_ops` also
-- serves. A maintenance target measured by its density: each long value
-- yields on the order of a thousand trigram keys, and vacuum removes dead
-- entries without merging entry-tree pages.
create index idx_ca_value_long_text_trgm on current_attributes using gin (attribute_name_key int4_ops, value_long_text gin_trgm_ops) with (fastupdate = off)
where
    value_long_text is not null;

-- A long Relationship object is too long to key a B-tree on, so a `q`
-- equality over it drives from the attribute key alone and filters the value
-- on the heap. It is not a maintenance target: a value update leaves its key
-- unchanged, so bottom-up index deletion removes the version churn as it
-- happens.
create index idx_ca_value_long_uri on current_attributes (attribute_name_key)
where
    value_long_uri is not null;

-- The containment arm of every `q` equality (clause 4.9), and the lookup of
-- the objects of a Relationship with several objects or of a
-- ListRelationship: `jsonb_path_ops` keys hashes, so no object is too long
-- for it.
create index idx_ca_value_object on current_attributes using gin (attribute_name_key int4_ops, value_object jsonb_path_ops) with (fastupdate = off)
where
    value_object is not null;

-- Geo-queries against a named GeoProperty (clause 4.10): every relation tests
-- this box before its exact predicate.
create index idx_ca_value_geometry on current_attributes using gist (attribute_name_key, coalesce(value_geometry_reach, value_geometry_valid, value_geometry))
where
    value_geometry is not null;

-- The planner reads no statistics from the expression of a partial index, so
-- without these it prices that box test at a fixed guess, however many
-- instances the box holds.
create statistics stx_ca_value_geometry on (coalesce(value_geometry_reach, value_geometry_valid, value_geometry))
from current_attributes;

-- The correlated instance lookup and the distance ordering of clause 4.23.
create index idx_ca_entity_geometry on current_attributes (attribute_name_key, entity_key)
where
    value_geometry is not null;

-- A clause 4.9 `q` equality on a sub-attribute, which reads the keys of its
-- operand and rechecks the value it compares.
create index idx_ca_sub_eq_keys on current_attributes using gin (sub_eq_keys) with (fastupdate = off)
where
    sub_eq_keys is not null;

-- The most common (attribute, kind) pairs show the planner how few of an
-- attribute's instances, often none, hold a given kind. A probe for whether
-- any instance of an attribute sets a column no B-tree orders (an object, a
-- long text, a long URI) names the kind the row pins to that column, and is
-- priced from this list rather than from the two columns taken independently.
create statistics stx_ca_attribute_value_kind (dependencies, ndistinct, mcv) on attribute_name_key, value_kind from current_attributes;

create statistics stx_ca_entity_attribute (dependencies, ndistinct) on entity_key, attribute_name_key from current_attributes;

-- The trigger's own schema names the Entities checked, because a Snapshot
-- schema's statements qualify its relations rather than steering the search
-- path.
create function check_current_attribute_entities ()
    returns trigger
    language plpgsql
    as $$
declare
    entity_keys bigint[];
    locked_count bigint;
begin
    entity_keys := array(select distinct written.entity_key from new_rows written);
    execute format('select count(*) from (select from %I.entities where entity_key = any ($1) for key share) locked', tg_table_schema) into locked_count
    using entity_keys;
    if locked_count < cardinality(entity_keys) then
        raise foreign_key_violation
        using message = 'current attribute of no entity', table = tg_table_name;
    end if;
    return null;
end
$$;

create trigger trg_current_attributes_entities_insert
    after insert on current_attributes referencing new table as new_rows for each statement
    execute function check_current_attribute_entities ();

create trigger trg_current_attributes_entities_update
    after update on current_attributes referencing new table as new_rows for each statement
    execute function check_current_attribute_entities ();

-- The deletion of Entities removes their attribute instances in one pass over
-- the grain, where a per-row cascade probes it once per Entity. The trigger's
-- own schema names the instances, as for the Entity check.
create function cascade_entity_current_attributes ()
    returns trigger
    language plpgsql
    as $$
begin
    execute format('delete from %I.current_attributes where entity_key = any ($1)', tg_table_schema)
    using array(select deleted.entity_key from old_rows deleted);
    return null;
end
$$;

create trigger trg_entities_current_attributes_cascade
    after delete on entities referencing old table as old_rows for each statement
    execute function cascade_entity_current_attributes ();
