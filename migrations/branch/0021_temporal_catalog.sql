-- ============================================================================
-- 0021: Temporal catalog
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.5.5, 4.17, 5.6.6
--
-- The catalog temporal history is recorded against: `temporal_entities` names
-- the Entity a Temporal Evolution belongs to, and `temporal_attribute_streams`
-- names the grain history is recorded at, one row per temporal entity,
-- attribute name and `datasetId` (clause 4.5.5).
--
-- Neither table references `entities`. Core Entity deletion (clause 5.6.6)
-- leaves temporal state standing, so a cascade from current state would delete
-- history the specification requires to survive; the Entity's URI is carried
-- here independently.
-- ============================================================================
create table temporal_entities (
    temporal_entity_key bigint generated always as identity (cache 32),
    created_at timestamptz not null,
    modified_at timestamptz not null,
    id text compression lz4 collate "C" not null,
    -- `entity_type_names` keys.
    entity_type_name_keys integer[] compression lz4 not null,
    constraint pk_temporal_entities primary key (temporal_entity_key),
    constraint uq_temporal_entities_id unique (id),
    constraint chk_temporal_entities_entity_type_name_keys_present check (cardinality(entity_type_name_keys) > 0),
    constraint chk_temporal_entities_id_length check (octet_length(id) <= 1024)
);

create index idx_temporal_entities_entity_type_name_keys on temporal_entities using gin (entity_type_name_keys);

create index idx_temporal_entities_id_trgm on temporal_entities using gin (id gin_trgm_ops);

-- One row per type a Temporal Evolution is recorded under (clause 4.17
-- selects on membership), keyed by type and then identifier, as
-- `entity_type_members` is for current state. A temporal type selection pages
-- by walking one range of this key in the clause 5.5.9 `id` order, so it
-- visits the Evolutions of that type and no other. The GIN over
-- `temporal_entities.entity_type_name_keys` cannot give that order, and a walk
-- of `uq_temporal_entities_id` filtered by type reads every Evolution whose
-- identifier sorts ahead of the type's last. The key carries
-- `temporal_entity_key` so the walk never visits the heap.
create table temporal_entity_type_members (
    temporal_entity_key bigint not null,
    entity_type_name_key integer not null,
    id text compression lz4 collate "C" not null,
    constraint pk_temporal_entity_type_members primary key (entity_type_name_key, id) include (temporal_entity_key),
    constraint fk_temporal_entity_type_members_temporal_entity foreign key (temporal_entity_key) references temporal_entities (temporal_entity_key) on delete cascade
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

-- The cascade from `temporal_entities` finds an Evolution's rows by key.
create index idx_temporal_entity_type_members_temporal_entity_key on temporal_entity_type_members (temporal_entity_key);

create table temporal_attribute_streams (
    -- Minted when the write path first inserts the grain and read back in the
    -- same transaction; never rendered.
    stream_key bigint generated always as identity (cache 32),
    temporal_entity_key bigint not null,
    -- The Evolution's identifier, which never changes, copied from
    -- `temporal_entities` when the stream is minted.
    id text compression lz4 collate "C" not null,
    attribute_name_key integer not null,
    dataset_id text compression lz4 collate "C",
    constraint pk_temporal_attribute_streams primary key (stream_key),
    -- `INCLUDE (stream_key)` makes stream resolution index-only: every append
    -- resolves its grain to a stream key, the hottest catalog read there is.
    constraint uq_temporal_attribute_streams_grain unique nulls not distinct (temporal_entity_key, attribute_name_key, dataset_id) include (stream_key),
    constraint fk_temporal_attribute_streams_temporal_entity foreign key (temporal_entity_key) references temporal_entities (temporal_entity_key) on delete cascade,
    constraint chk_temporal_attribute_streams_dataset_id_length check (octet_length(dataset_id) <= 1024)
);

-- A temporal selection by attribute (clause 5.7.4.4 S1, `attrs` with no type)
-- pages the Evolutions holding a stream of the attribute by walking one range
-- of this key in the clause 5.5.9 `id` order, so it visits those Evolutions and
-- no other. The grain key is led by the Evolution and cannot give that order,
-- and a walk of `uq_temporal_entities_id` probing each Evolution's grain reads
-- every Evolution whose identifier sorts ahead of the attribute's last holder.
-- The key lives on the stream itself, so whatever deletes a stream (an
-- attribute, a window of history, the Evolution) takes its entry with it.
-- The Evolution's surrogate key rides along so a count can re-order the
-- holders by it from the index alone: probed in that order, their history
-- reads follow the indexes, where in identifier order they read at random.
create index idx_temporal_attribute_streams_attribute_name_key_id on temporal_attribute_streams (attribute_name_key, id) include (temporal_entity_key);

