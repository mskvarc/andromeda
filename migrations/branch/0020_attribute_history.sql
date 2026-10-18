-- ============================================================================
-- 0020: Attribute history
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.5.0, 4.5.7, 4.5.8, 4.9, 4.11,
--   5.2.21, 5.6.14, 5.6.15, 5.7.4
--
-- One row per recorded attribute instance. The live copy is a hypertable
-- partitioned on `recorded_at`; a Snapshot's copy stays a plain table, so every
-- statement here holds for both shapes.
--
-- Codes:
--
--   attribute_kind: 1 Property, 2 Relationship, 3 GeoProperty,
--     4 VocabProperty, 5 LanguageProperty, 6 JsonProperty, 7 ListProperty,
--     8 ListRelationship
--   value_kind: 0 tombstone, 1 number, 2 string, 3 boolean, 4 geometry,
--     5 object, 6 date-time, 7 uri
--
-- `recorded_at` is the write clock and is never rendered. `observed_at`,
-- `created_at`, `modified_at` and `deleted_at` are the four axes clause 5.2.21
-- admits as a `timeproperty`, each written only from what was observed, so a
-- time filter selects exactly the requested column: a row with no `observedAt`
-- matches no `observedAt` filter (clause 4.11).
--
-- A geometry keeps its `bbox` and its foreign members (IETF RFC 7946 sections 5
-- and 6.1) beside its positions in `value_geometry_members`, as the expanded
-- members of its `GeoJSON` node, so a history row renders the geometry it
-- recorded (clause 4.6.4); the column holds nothing for any other value.
--
-- A tombstone carries `deleted_at` and no typed value; its NGSI-LD null is
-- synthesised at render time, never stored. `attribute_kind` is kept so the
-- null takes its family's shape: a value for a Property, an object for a
-- Relationship, a language map for a LanguageProperty (clauses 4.5.0, 4.5.7,
-- 4.5.8).
--
-- What an instance carries beside its value is stored as current state stores
-- it: `value_type_key`, `object_type_keys`, `unit_code_id`, `expires_at`,
-- `proof_seals`, and its sub-attributes as `sub_attribute` elements whose
-- `createdAt` and `modifiedAt` are NULL (history records the system times of
-- the instance alone). A tombstone holds none of them. No foreign key reaches
-- the dictionary keys, as none reaches `stream_key`: the dictionaries are
-- append-only, and a key is minted in the transaction that writes the row.
--
-- An instance identifier sits in one of two columns: a broker-minted one as its
-- UUID, rendered through the stable template
-- `urn:ngsi-ld:attribute:instance:{uuid}`; a client-supplied one exactly as
-- received, because clause 4.5.7 hands it back to the client.
--
-- `stream_key` carries no foreign key. TimescaleDB re-adds a hypertable's
-- foreign keys on every chunk under `SHARE ROW EXCLUSIVE`, which deadlocks
-- against concurrent upserts on the referenced table. `unit_code_id`
-- references nothing in the database: the identity is the `cefact-units`
-- assignment.
--
-- The table has no spatial index. Clause 5.7.4.4 checks a geoquery against
-- the `GeoProperty` instances of each Entity it selects, so every spatial read
-- is one Evolution's rows, reached through the stream indexes. A geometry
-- index over the whole table is only ever ANDed into that lookup, and its
-- bitmap costs far more than filtering the few rows.
--
-- An insert-driven vacuum runs once a twentieth of the rows are new rather
-- than the default fifth. A hypertable chunk copies the parameter when it is
-- created, so the chunk receiving appends is vacuumed in smaller steps.
-- ============================================================================
create table attribute_history (
    recorded_at timestamptz not null,
    write_id uuid not null,
    stream_key bigint not null,
    write_ordinal integer not null,
    attribute_kind smallint not null,
    value_kind smallint not null,
    observed_at timestamptz,
    created_at timestamptz,
    modified_at timestamptz,
    deleted_at timestamptz,
    value_date_time timestamptz,
    -- The instance's clause 4.22 `expiresAt`, as it was recorded.
    expires_at timestamptz,
    generated_instance_id uuid,
    -- `entity_type_names` key of the instance's clause 4.5.2 `valueType`.
    value_type_key integer,
    unit_code_id smallint,
    value_boolean boolean,
    verbatim_instance_id text compression lz4 collate "C",
    value_number numeric compression lz4,
    value_string text compression lz4 collate "C",
    value_geometry geometry compression lz4,
    value_geometry_members jsonb compression lz4,
    value_object jsonb compression lz4,
    value_uri text compression lz4 collate "C",
    -- `entity_type_names` keys of the clause 4.5.3 `objectType` list, in
    -- written order.
    object_type_keys integer[] compression lz4,
    -- The clause 4.5.2.2 `entityIdSealed` and `entityTypeSealed` members of
    -- the instance, NULL when it carries neither.
    proof_seals jsonb compression lz4,
    -- The instance's sub-attributes, NULL when it has none.
    sub_attributes sub_attribute[] compression lz4,
    constraint uq_attribute_history_write_identity unique (recorded_at, write_id, write_ordinal),
    constraint chk_attribute_history_write_ordinal check (write_ordinal >= 0),
    constraint chk_attribute_history_attribute_kind check (attribute_kind between 1 and 8),
    constraint chk_attribute_history_unit_code_id_positive check (unit_code_id > 0),
    constraint chk_attribute_history_instance_identity check (num_nonnulls (generated_instance_id, verbatim_instance_id) = 1),
    constraint chk_attribute_history_verbatim_instance_id_length check (octet_length(verbatim_instance_id) <= 1024),
    constraint chk_attribute_history_value_presence check ((value_kind = 0 and deleted_at is not null and num_nonnulls (value_number, value_string, value_boolean, value_geometry, value_object, value_date_time, value_uri) = 0) or (value_kind <> 0 and num_nonnulls (value_number, value_string, value_boolean, value_geometry, value_object, value_date_time, value_uri) = 1)),
    -- Pins `value_kind` to the populated column, so a value whose declared
    -- shape disagrees with where it sits cannot be stored and is never
    -- repaired by guessing. It also bounds `value_kind` to its codes.
    constraint chk_attribute_history_value_kind_column check (value_kind = 0 or (value_kind = 1 and value_number is not null) or (value_kind = 2 and value_string is not null) or (value_kind = 3 and value_boolean is not null) or (value_kind = 4 and value_geometry is not null) or (value_kind = 5 and value_object is not null) or (value_kind = 6 and value_date_time is not null) or (value_kind = 7 and value_uri is not null)),
    constraint chk_attribute_history_value_geometry_valid check (value_geometry is null or st_isvalid(value_geometry)),
    constraint chk_attribute_history_value_geometry_srid check (value_geometry is null or st_srid(value_geometry) = 4326),
    constraint chk_attribute_history_value_geometry_zm check (value_geometry is null or st_zmflag(value_geometry) in (0, 2)),
    constraint chk_attribute_history_value_geometry_members check (value_geometry_members is null or (value_geometry is not null and jsonb_typeof(value_geometry_members) = 'object')),
    constraint chk_attribute_history_tombstone_members check (value_kind <> 0 or num_nonnulls (value_type_key, object_type_keys, unit_code_id, expires_at, proof_seals, sub_attributes) = 0),
    constraint chk_attribute_history_object_type_keys_present check (object_type_keys is null or cardinality(object_type_keys) > 0),
    constraint chk_attribute_history_sub_attributes_present check (sub_attributes is null or cardinality(sub_attributes) > 0)
)
with (
    autovacuum_vacuum_insert_scale_factor = 0.05
);

create index idx_attribute_history_stream_recorded_at on attribute_history (stream_key, recorded_at desc);

create index idx_attribute_history_stream_observed_at on attribute_history (stream_key, observed_at desc)
where
    observed_at is not null;

-- A deletion with no `observedAt` is placed at its `deleted_at`; the
-- instance-in-effect probe of a temporal `q` seeks the latest one per stream.
-- The predicate names `observed_at` so that a generic plan, which cannot see
-- the bound instant, still prices the index by how few rows it holds.
create index idx_attribute_history_stream_deletion on attribute_history (stream_key, deleted_at desc)
where
    deleted_at is not null and observed_at is null;

-- The broker stamps `createdAt` and `modifiedAt` itself (clause 4.8), and an
-- appended row records both at its write instant, so a stream's rows in
-- `created_at` or `modified_at` order are, with few exceptions, its rows in
-- `recorded_at` order. A page on either axis reads those rows through the
-- write-clock index above, and the exceptions through these: a modified
-- instance, a row a Snapshot captured from a Context Source with the
-- source's own system times, or one recorded without them. On a write that
-- appends, neither takes an entry.
create index idx_attribute_history_stream_created_apart on attribute_history (stream_key, created_at desc)
where
    created_at is distinct from recorded_at;

create index idx_attribute_history_stream_modified_apart on attribute_history (stream_key, modified_at desc)
where
    modified_at is distinct from recorded_at;

create index idx_attribute_history_stream_generated_instance on attribute_history (stream_key, generated_instance_id)
where
    generated_instance_id is not null;

-- A lookup only; `attribute_history_instance_claims` carries the uniqueness.
create index idx_attribute_history_stream_verbatim_instance on attribute_history (stream_key, verbatim_instance_id)
where
    verbatim_instance_id is not null;

-- One BRIN summary per semantic time axis. A window on `recorded_at` is
-- answered by chunk exclusion, and the B-trees above lead with `stream_key`,
-- so a query that bounds `observedAt`, `createdAt`, `modifiedAt` or
-- `deletedAt` without naming a stream, which clause 5.7.4 permits, has the
-- summary as its only access path short of scanning every chunk.
--
-- `timestamptz_minmax_multi_ops` rather than the single-range default: an
-- observation arriving a year late would stretch a single per-block range
-- across that year and make the block match every query, while a multi-range
-- summary keeps the outlier in its own range.
--
-- Not partial. BRIN summarises NULLs per block rather than indexing them one
-- by one, so a `WHERE ... IS NOT NULL` predicate saves nothing.
--
-- A range covers 32 pages rather than 128, so a one-day window reads fewer
-- blocks beside its own for an index still a small fraction of the heap.
-- `autosummarize` summarises a range as soon as appends move past it. An
-- unsummarised range matches every query, so without it the newest history
-- would stay unskippable until a vacuum reached the table.
create index idx_attribute_history_observed_at_brin on attribute_history using brin (observed_at timestamptz_minmax_multi_ops) with (pages_per_range = 32, autosummarize = on);

create index idx_attribute_history_created_at_brin on attribute_history using brin (created_at timestamptz_minmax_multi_ops) with (pages_per_range = 32, autosummarize = on);

create index idx_attribute_history_modified_at_brin on attribute_history using brin (modified_at timestamptz_minmax_multi_ops) with (pages_per_range = 32, autosummarize = on);

create index idx_attribute_history_deleted_at_brin on attribute_history using brin (deleted_at timestamptz_minmax_multi_ops) with (pages_per_range = 32, autosummarize = on);

-- Clauses 5.6.14 and 5.6.15 name an instance by Entity, attribute and
-- `instanceId` only, so a client-supplied one must be unique across one
-- attribute of one Entity, whichever `datasetId` carries it. The history table
-- cannot enforce that: its rows name a stream rather than the Entity and
-- attribute, and a unique index on a hypertable must contain `recorded_at`,
-- which would let two rows claiming the same identifier at different
-- instants both pass. This table carries the invariant instead, keyed by
-- Entity and attribute rather than by stream, so two sibling `datasetId`
-- streams claiming one identifier collide as they must.
--
-- A claim is taken when a history row with a client-supplied identifier is
-- written, and released when that row is removed, by the delete that
-- precedes a replacing write or by instance deletion (clause 5.6.15). A
-- broker-minted identifier is unique by construction and never claims a row.
create table attribute_history_instance_claims (
    temporal_entity_key bigint not null,
    attribute_name_key integer not null,
    verbatim_instance_id text compression lz4 collate "C" not null,
    constraint pk_attribute_history_instance_claims primary key (temporal_entity_key, attribute_name_key, verbatim_instance_id),
    constraint fk_attribute_history_instance_claims_temporal_entity foreign key (temporal_entity_key) references temporal_entities (temporal_entity_key) on delete cascade,
    constraint chk_attribute_history_instance_claims_id_length check (octet_length(verbatim_instance_id) <= 1024)
);

