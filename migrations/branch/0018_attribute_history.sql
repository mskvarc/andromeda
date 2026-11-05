-- ============================================================================
-- 0018: Attribute history
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
--     5 object, 6 date-time, 7 uri, 8 date, 9 time, 10 declared-number,
--     11 declared-boolean
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
-- A scalar keeps the JSON-LD value object it was written as, as current state
-- keeps it: `value_datatype_key` is the `entity_type_names` key of the `@type`
-- a typed number, string, boolean or time value declares, `value_language` the
-- `@language` of a language-tagged string, so the Temporal Evolution renders
-- the instance current state renders. A JSON string declared as an XSD number
-- or `xsd:boolean` is held as current state holds it, in `value_number` (kind
-- 10) or `value_boolean` (kind 11), naming the datatype that declares it.
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
    value_datatype_key integer,
    unit_code_id smallint,
    value_boolean boolean,
    verbatim_instance_id text compression lz4 collate "C",
    value_number numeric compression lz4,
    value_string text compression lz4 collate "C",
    value_language text compression lz4 collate "C",
    value_geometry geometry compression lz4,
    -- The `ST_MakeValid` form of an invalid `value_geometry`, NULL when the
    -- written one is valid, held beside the written form as on
    -- `current_attributes`.
    value_geometry_valid geometry compression lz4,
    -- The great-circle reach of `value_geometry`, as on `current_attributes`:
    -- the box a geo-query tests an Evolution's rows by before the exact
    -- relation.
    value_geometry_reach geometry compression lz4,
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
    constraint chk_attribute_history_write_ordinal check (write_ordinal >= 0),
    constraint chk_attribute_history_attribute_kind check (attribute_kind between 1 and 8),
    constraint chk_attribute_history_unit_code_id_positive check (unit_code_id > 0),
    constraint chk_attribute_history_instance_identity check (num_nonnulls(generated_instance_id, verbatim_instance_id) = 1),
    constraint chk_attribute_history_verbatim_instance_id_length check (octet_length(verbatim_instance_id) <= 1024),
    constraint chk_attribute_history_value_presence check ((value_kind = 0 and deleted_at is not null and num_nonnulls(value_number, value_string, value_boolean, value_geometry, value_object, value_date_time, value_uri) = 0) or (value_kind <> 0 and num_nonnulls(value_number, value_string, value_boolean, value_geometry, value_object, value_date_time, value_uri) = 1)),
    -- Pins `value_kind` to the populated column, so a value whose declared
    -- shape disagrees with where it sits cannot be stored and is never
    -- repaired by guessing. It also bounds `value_kind` to its codes.
    constraint chk_attribute_history_value_kind_column check (value_kind = 0 or (value_kind = 1 and value_number is not null) or (value_kind = 2 and value_string is not null) or (value_kind = 3 and value_boolean is not null) or (value_kind = 4 and value_geometry is not null) or (value_kind = 5 and value_object is not null) or (value_kind = 6 and value_date_time is not null) or (value_kind = 7 and value_uri is not null) or (value_kind = 8 and value_string is not null) or (value_kind = 9 and value_string is not null) or (value_kind = 10 and value_number is not null and num_nonnulls(value_datatype_key, value_type_key) > 0) or (value_kind = 11 and value_boolean is not null and num_nonnulls(value_datatype_key, value_type_key) > 0)),
    constraint chk_attribute_history_value_datatype_with_scalar check (value_datatype_key is null or value_kind in (1, 2, 3, 6, 8, 9, 10, 11)),
    constraint chk_attribute_history_value_language_with_string check (value_language is null or (value_kind = 2 and value_datatype_key is null)),
    constraint chk_attribute_history_value_geometry_valid check (value_geometry_valid is null or (value_geometry is not null and st_isvalid(value_geometry_valid) and not st_isvalid(value_geometry))),
    constraint chk_attribute_history_value_geometry_srid check (value_geometry is null or st_srid(value_geometry) = 4326),
    constraint chk_attribute_history_value_geometry_zm check (value_geometry is null or st_zmflag(value_geometry) in (0, 2)),
    constraint chk_attribute_history_value_geometry_reach check (value_geometry_reach is null or (value_geometry is not null and st_srid(value_geometry_reach) = 4326 and value_geometry_reach ~ value_geometry)),
    constraint chk_attribute_history_value_geometry_members check (value_geometry_members is null or (value_geometry is not null and jsonb_typeof(value_geometry_members) = 'object')),
    constraint chk_attribute_history_tombstone_members check (value_kind <> 0 or num_nonnulls(value_type_key, object_type_keys, unit_code_id, expires_at, proof_seals, sub_attributes) = 0),
    constraint chk_attribute_history_object_type_keys_present check (object_type_keys is null or cardinality(object_type_keys) > 0),
    constraint chk_attribute_history_sub_attributes_present check (sub_attributes is null or cardinality(sub_attributes) > 0)
)
with (
    autovacuum_vacuum_insert_scale_factor = 0.05
);

-- Every read of a stream on a time axis is a range of its rows that carry an
-- instant there, ordered on that instant and then on the write that recorded
-- the row: a window, a clause 6.3.10 page and a `lastN` (clauses 5.7.3.4 and
-- 5.7.4.4) on `observedAt`, `createdAt`, `modifiedAt` or `deletedAt`. Each
-- axis has one stream-led index holding exactly the rows that carry it, so a
-- stream with no instance on the axis holds no entry to read. The
-- `observedAt` and `deletedAt` indexes are keyed in that whole order, so a
-- read limited to n instances reads n entries however many instances share
-- an instant. The `createdAt` and `modifiedAt` indexes key the instant alone:
-- every instance one write records holds the write instant on both, a key
-- with no tie-breaker keeps those ties in one deduplicated entry, and a read
-- on either axis sorts the ties of one write instant above the scan. A row
-- with no instant on an axis is represented by no read on it (clause
-- 5.2.21), so leaving it out of the index loses nothing. A bound on the
-- column, which every read states, is also what excludes a columnstore chunk
-- by its range on the axis.
--
-- The instance a stream held when a window opens (clause 4.11) is its latest
-- row at or before the start: a row at its instant on the window's axis, or a
-- deletion carrying none there at its `deleted_at` (clause 4.5.7). It is read
-- as the later of the two heads, one from the axis index and one from the
-- `deletedAt` index among the deletions without the axis, each stopped after
-- the rows tied with its first. `deletedAt` opens no window with an instance
-- in effect.
--
-- A row lands in at most the three indexes of the axes it carries: an
-- observation in `createdAt`, `modifiedAt` and, when observed, `observedAt`
-- (the broker stamps the first two on every instance, clause 4.8), a
-- deletion in `deletedAt` alone.
--
-- The `createdAt` and `modifiedAt` keys ascend. A stream receives the write
-- instant in increasing order, so a new entry lands at the end of its
-- stream's run, and PostgreSQL splits a full leaf of keys no wider than two
-- eight-byte columns at that entry, leaving the left page full. A descending
-- key places it at the start of the run, where every split halves the page.
-- The `observedAt` and `deletedAt` keys are too wide for that split choice,
-- so their direction leaves their size unchanged, and they descend in the
-- order a `lastN` reads.
create index idx_attribute_history_stream_observed_at on attribute_history (stream_key, observed_at desc, write_id desc, write_ordinal desc)
where
    observed_at is not null;

create index idx_attribute_history_stream_created_at on attribute_history (stream_key, created_at)
where
    created_at is not null;

create index idx_attribute_history_stream_modified_at on attribute_history (stream_key, modified_at)
where
    modified_at is not null;

create index idx_attribute_history_stream_deleted_at on attribute_history (stream_key, deleted_at desc, write_id desc, write_ordinal desc)
where
    deleted_at is not null;

create index idx_attribute_history_stream_generated_instance on attribute_history (stream_key, generated_instance_id)
where
    generated_instance_id is not null;

-- A lookup only; `attribute_history_instance_claims` carries the uniqueness.
-- A replacing write removes the row it replaces through it.
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
    entity_key bigint not null,
    attribute_name_key integer not null,
    verbatim_instance_id text compression lz4 collate "C" not null,
    constraint pk_attribute_history_instance_claims primary key (entity_key, attribute_name_key, verbatim_instance_id),
    constraint fk_attribute_history_instance_claims_entity foreign key (entity_key) references entities (entity_key) on delete cascade,
    constraint chk_attribute_history_instance_claims_id_length check (octet_length(verbatim_instance_id) <= 1024)
);
