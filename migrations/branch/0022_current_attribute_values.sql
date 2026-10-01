-- ============================================================================
-- 0022: Current attribute values
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.5.5, 4.6.3, 4.9, 4.10, 4.22, 4.23,
-- 5.6.6
--
-- The value each attribute instance of each Entity currently has, one row per
-- Entity, attribute name and `datasetId` (clause 4.5.5), in one typed column
-- per value shape so a `q` comparison reads a single indexed column. The only
-- ownership edge is to `entities`: Core Entity deletion (clause 5.6.6) leaves
-- history standing, so nothing here references the temporal catalog.
--
-- Codes:
--
--   attribute_kind: 1 Property, 2 Relationship, 3 GeoProperty,
--     4 VocabProperty, 5 LanguageProperty, 6 JsonProperty, 7 ListProperty,
--     8 ListRelationship
--   value_kind: 1 number, 2 string, 3 boolean, 4 geometry, 5 object,
--     6 date-time, 7 uri
--
-- Value kind 0, the tombstone, never appears here: a deleted attribute is an
-- absent row, not a row carrying a marker.
--
-- Clause 4.6.3 names three data types, DateTime, Date and Time, whose whole
-- representation is the string, so a value written in one of their lexical
-- forms is projected as that data type. A DateTime lands in
-- `value_date_time`. A Date or a Time lands in `value_string` in its
-- canonical fixed-width form (`YYYY-MM-DD`, `HH:MM:SS.ffffffZ`), so byte order
-- is chronological order and a clause 4.9 comparison orders it correctly.
--
-- `expires_at` is the instance's own clause 4.22 `expiresAt`, NULL when the
-- instance never expires. Clause 4.22 makes an attribute "invalid" at that
-- instant while letting its deletion lag ("final deletion will always lag the
-- expiresAt timestamp to a certain extent"), so every read of this table
-- excludes a past-due instance with `expires_at IS NULL OR expires_at > NOW()`
-- and the attribute-expiry sweep deletes the row later. The sweep finds its
-- candidates through `entities.min_attribute_expires_at`, the earliest of these
-- instants per Entity computed from the same payload, so the column carries no
-- index of its own: it is only ever read beside a row another index found, and
-- leaving it unindexed keeps an expiry-only rewrite of a row heap-only.
--
-- There is no `modified_at`: nothing reads when a projected value was last
-- written, since `entity_documents.modified_at` carries the Entity's instant.
-- A per-row instant would turn every restatement of an unchanged value into a
-- real row rewrite, which is the write the upsert's no-op guard avoids: an
-- instance whose projection is value-identical is not rewritten at all. The
-- guard compares with each column type's own equality, under which `NUMERIC`
-- ignores scale (`1.0 = 1.00`); no reader renders a projected number, it only
-- compares it.
-- ============================================================================
create table current_attribute_values (
    entity_key bigint not null,
    -- `attribute_names` key.
    attribute_name_key integer not null,
    attribute_kind smallint not null,
    value_kind smallint not null,
    value_date_time timestamptz,
    expires_at timestamptz,
    unit_code_id smallint,
    value_boolean boolean,
    dataset_id text compression lz4 collate "C",
    -- `NUMERIC`, not `DOUBLE PRECISION`: JSON numbers are stored exactly, and
    -- a comparison beyond 2^53 stays correct without falling back to the
    -- payload.
    value_number numeric compression lz4,
    value_string text compression lz4 collate "C",
    value_geometry geometry compression lz4,
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
    constraint fk_current_attribute_values_entity foreign key (entity_key) references entities (entity_key) on delete cascade,
    -- The grain has a nullable `dataset_id`, so it is a unique constraint and
    -- the table has no primary key. It is the upsert's conflict target and the
    -- access path of every Entity-correlated `q` probe.
    constraint uq_current_attribute_values_grain unique nulls not distinct (entity_key, attribute_name_key, dataset_id),
    constraint chk_current_attribute_values_attribute_kind check (attribute_kind between 1 and 8),
    constraint chk_current_attribute_values_value_presence check (num_nonnulls (value_number, value_string, value_boolean, value_geometry, value_object, value_date_time, value_uri, value_long_text, value_long_uri) = 1),
    constraint chk_current_attribute_values_value_kind_column check ((value_kind = 1 and value_number is not null) or (value_kind = 2 and num_nonnulls (value_string, value_long_text) = 1) or (value_kind = 3 and value_boolean is not null) or (value_kind = 4 and value_geometry is not null) or (value_kind = 5 and value_object is not null) or (value_kind = 6 and value_date_time is not null) or (value_kind = 7 and num_nonnulls (value_uri, value_long_uri) = 1)),
    constraint chk_current_attribute_values_value_geometry_valid check (value_geometry is null or st_isvalid(value_geometry)),
    constraint chk_current_attribute_values_value_geometry_srid check (value_geometry is null or st_srid(value_geometry) = 4326),
    constraint chk_current_attribute_values_value_geometry_zm check (value_geometry is null or st_zmflag(value_geometry) in (0, 2)),
    constraint chk_current_attribute_values_value_string_length check (octet_length(value_string) <= 2048),
    constraint chk_current_attribute_values_value_long_text_length check (octet_length(value_long_text) > 2048),
    constraint chk_current_attribute_values_value_uri_length check (octet_length(value_uri) <= 1024),
    constraint chk_current_attribute_values_value_long_uri_length check (octet_length(value_long_uri) > 1024),
    constraint chk_current_attribute_values_dataset_id_length check (octet_length(dataset_id) <= 1024)
)
with (
    fillfactor = 80,
    autovacuum_vacuum_scale_factor = 0.005,
    autovacuum_vacuum_threshold = 500,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 500,
    autovacuum_vacuum_cost_delay = 0
);

-- One partial B-tree per typed column, led by the attribute key, for plans
-- that drive from the attribute side.
create index idx_cav_value_number on current_attribute_values (attribute_name_key, value_number)
where
    value_number is not null;

create index idx_cav_value_string on current_attribute_values (attribute_name_key, value_string)
where
    value_string is not null;

-- Unanchored `~=` and `!~=` patterns (clause 4.9). `fastupdate = off`: the
-- projection is written inside the Entity's transaction, and a GIN pending
-- list would put its flush latency on the request path.
create index idx_cav_value_string_trgm on current_attribute_values using gin (attribute_name_key int4_ops, value_string gin_trgm_ops) with (fastupdate = off)
where
    value_string is not null;

create index idx_cav_value_boolean on current_attribute_values (attribute_name_key, value_boolean)
where
    value_boolean is not null;

create index idx_cav_value_date_time on current_attribute_values (attribute_name_key, value_date_time)
where
    value_date_time is not null;

create index idx_cav_value_uri on current_attribute_values (attribute_name_key, value_uri)
where
    value_uri is not null;

-- A long String is too long to key a B-tree on, so a `q` comparison over it
-- drives from the attribute key alone and filters the value on the heap.
create index idx_cav_value_long_text on current_attribute_values (attribute_name_key)
where
    value_long_text is not null;

-- Patterns over long Strings, and their equality, which `gin_trgm_ops` also
-- serves. Neither long-text index is a maintenance target: they cover rare
-- values, and vacuuming the table reclaims their dead entries.
create index idx_cav_value_long_text_trgm on current_attribute_values using gin (attribute_name_key int4_ops, value_long_text gin_trgm_ops) with (fastupdate = off)
where
    value_long_text is not null;

-- A long Relationship object is too long to key a B-tree on, so a `q`
-- equality over it drives from the attribute key alone and filters the value
-- on the heap. Like the long-text indexes it covers rare values and is not a
-- maintenance target.
create index idx_cav_value_long_uri on current_attribute_values (attribute_name_key)
where
    value_long_uri is not null;

-- The containment arm of every `q` equality (clause 4.9).
create index idx_cav_value_object on current_attribute_values using gin (attribute_name_key int4_ops, value_object jsonb_path_ops) with (fastupdate = off)
where
    value_object is not null;

-- Geo-queries against a named GeoProperty (clause 4.10).
create index idx_cav_value_geometry on current_attribute_values using gist (attribute_name_key, value_geometry)
where
    value_geometry is not null;

-- The correlated instance lookup and the distance ordering of clause 4.23.
create index idx_cav_entity_geometry on current_attribute_values (attribute_name_key, entity_key)
where
    value_geometry is not null;

create statistics stx_cav_attribute_value_kind (dependencies, ndistinct) on attribute_name_key, value_kind from current_attribute_values;

create statistics stx_cav_entity_attribute (dependencies, ndistinct) on entity_key, attribute_name_key from current_attribute_values;

