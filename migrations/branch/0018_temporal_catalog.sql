-- ============================================================================
-- 0018: Temporal catalog
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.5.5, 4.5.6, 5.6.6
--
-- The streams temporal history is recorded at: one row per Entity,
-- attribute name and `datasetId` (clause 4.5.5), beside the extent of the
-- geometries each stream recorded and the families and value shapes it holds.
-- The Entity a Temporal Evolution belongs to is its `entities` row, which a
-- core deletion (clause 5.6.6) keeps while the Evolution exists.
-- ============================================================================
create table temporal_attribute_streams (
    -- Minted when the write path first inserts the grain and read back in the
    -- same transaction; never rendered.
    stream_key bigint generated always as identity (cache 32),
    entity_key bigint not null,
    attribute_name_key integer not null,
    dataset_id text compression lz4 collate "C",
    -- The bounding box of every geometry the stream ever recorded, null until
    -- it records one. The write path grows it when a recorded geometry falls
    -- outside it and never shrinks it, so after a deletion, an instance
    -- modification or a dropped chunk it still holds every geometry the
    -- stream holds.
    value_extent geometry compression lz4,
    -- Every combination of attribute family and value shape the stream's
    -- instances hold, tombstones excepted (a deletion carries no value), one
    -- bit each. Clause 5.7.4.4 refuses an aggregation over an attribute that
    -- is not eligible for a requested method, and Tables 4.5.19.1-1 to
    -- 4.5.19.1-3 decide eligibility from both: the family selects the table,
    -- the shape its column. Read here, the decision touches no history row,
    -- however long the history is.
    --
    -- The shape is finer than `attribute_history.value_kind`: an object and an
    -- array share one JSONB column, and a string, a `Date` and a `Time` one
    -- text column, while the tables give each a different set of methods.
    --
    -- The combination of family code k and shape code s is bit
    -- (k - 1) * 10 + (s - 1), counted from the left as `get_bit` counts:
    --
    --   attribute_kind: 1 Property, 2 Relationship, 3 GeoProperty,
    --     4 VocabProperty, 5 LanguageProperty, 6 JsonProperty, 7 ListProperty,
    --     8 ListRelationship
    --   recorded_shape: 1 number, 2 string, 3 boolean, 4 geometry, 5 object,
    --     6 array, 7 date-time, 8 date, 9 time, 10 uri
    --
    -- The write path sets the bit of a combination it records an instance of
    -- when the mask lacks it; in the steady state the mask holds it and the
    -- write leaves the row alone. A path that removes or modifies instances
    -- clears the bits no instance of the stream carries any longer, holding
    -- the Temporal Evolution's row lock that every recording write takes
    -- first, so a concurrent write of the same combination has either
    -- committed, and the clearing reads its instance, or sets the bit again
    -- once the clearing has committed.
    shape_mask bit(80) compression lz4 not null default b'0'::bit(80),
    constraint pk_temporal_attribute_streams primary key (stream_key),
    constraint fk_temporal_attribute_streams_entity foreign key (entity_key) references entities (entity_key) on delete cascade,
    constraint chk_temporal_attribute_streams_dataset_id_length check (octet_length(dataset_id) <= 1024),
    -- The empty string is the grain's key for the default instance, so no
    -- `datasetId` may be it.
    constraint chk_temporal_attribute_streams_dataset_id_not_empty check (dataset_id <> '')
);

-- The grain, its default instance keyed as the empty string. A grain compared
-- with `is not distinct from` is joined by testing every pair of rows, so
-- resolving the streams of one write costs the product of the grains it
-- names and the streams its Entity holds; keyed by `coalesce`, the same join
-- is a hash join or an index probe. Every statement spells the grain as this
-- expression. `include (stream_key, dataset_id)` makes stream resolution
-- index-only: every append resolves its grain to a stream key, the hottest
-- catalog read there is, and a read of an Entity's streams returns each
-- stream's `datasetId`, which the keyed expression alone does not give back.
create unique index uq_temporal_attribute_streams_grain on temporal_attribute_streams (entity_key, attribute_name_key, (coalesce(dataset_id, ''))) include (stream_key, dataset_id);

-- The holders of an attribute, in the order of their surrogate keys: a
-- temporal selection by attribute (clause 5.7.4.4 S1, `attrs` with no type)
-- whose attribute has few holders reads them here and sorts them by
-- identifier, and a count probes their history in this order, which follows
-- the history indexes, where in identifier order the probes read at random.
-- An attribute with many holders is paged through the Evolution memberships
-- of the types `stream_type_census` names instead, so no stream carries the
-- Entity's identifier.
--
-- It keys the grain's `datasetId` last. Both indexes then match a grain
-- lookup in full, so a planner that estimates one stream per Entity and
-- attribute, and takes whichever index is smaller, still probes one stream
-- rather than reading every stream of the attribute.
create index idx_temporal_attribute_streams_attribute_name_key_entity_key on temporal_attribute_streams (attribute_name_key, entity_key, (coalesce(dataset_id, '')));

-- A temporal geo query (clause 5.7.4.4 S3) reads its candidate Evolutions
-- here: a stream whose extent misses the relation's box holds no geometry
-- that can meet the relation, whatever the window. The extents are one row a
-- stream, where the history's geometries sit in converted chunks no spatial
-- index reaches. Partial, so a stream that never recorded a geometry costs
-- the index nothing.
create index idx_temporal_attribute_streams_value_extent on temporal_attribute_streams using gist (value_extent)
where
    value_extent is not null;

-- Eligibility walks the distinct masks of an attribute, one descent each,
-- however many streams share a mask, and reaches the streams holding a
-- combination through the masks that hold it.
create index idx_temporal_attribute_streams_attribute_name_key_shape_mask on temporal_attribute_streams (attribute_name_key, shape_mask);

-- How many streams of each attribute the Temporal Evolutions of each type
-- hold, as a signed-delta ledger like the discovery census: a temporal
-- selection by attribute alone pages the holders by walking the Evolution
-- memberships of every type holding the attribute, which no other relation
-- names without reading every stream. A stream counts under every type its
-- Evolution is recorded under. Every statement that inserts or deletes
-- streams adds the change from its own `RETURNING`, under the types it reads
-- from the identity row it locks, and a type an Evolution gains counts the
-- streams the Evolution holds once the row is locked, so a stream and a type
-- written concurrently are counted together exactly once. A writer only ever
-- inserts, so no counter row is shared, and the census fold keeps the ledger
-- near one row per key.
create table stream_type_census (
    entity_type_name_key integer not null,
    attribute_name_key integer not null,
    stream_delta bigint not null,
    constraint chk_stream_type_census_stream_delta check (stream_delta <> 0)
)
with (
    vacuum_truncate = false,
    autovacuum_vacuum_insert_scale_factor = 0,
    autovacuum_vacuum_insert_threshold = 500,
    autovacuum_vacuum_scale_factor = 0,
    autovacuum_vacuum_threshold = 200,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 200,
    autovacuum_vacuum_cost_delay = 0
);

-- The holding types of the attributes a selection names, read by attribute.
create index idx_stream_type_census_attribute_name_key on stream_type_census (attribute_name_key, entity_type_name_key);
