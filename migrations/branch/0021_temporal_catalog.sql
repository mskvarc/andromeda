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

create table temporal_attribute_streams (
    -- Minted when the write path first inserts the grain and read back in the
    -- same transaction; never rendered.
    stream_key bigint generated always as identity (cache 32),
    temporal_entity_key bigint not null,
    attribute_name_key integer not null,
    dataset_id text compression lz4 collate "C",
    constraint pk_temporal_attribute_streams primary key (stream_key),
    -- `INCLUDE (stream_key)` makes stream resolution index-only: every append
    -- resolves its grain to a stream key, the hottest catalog read there is.
    constraint uq_temporal_attribute_streams_grain unique nulls not distinct (temporal_entity_key, attribute_name_key, dataset_id) include (stream_key),
    constraint fk_temporal_attribute_streams_temporal_entity foreign key (temporal_entity_key) references temporal_entities (temporal_entity_key) on delete cascade,
    constraint chk_temporal_attribute_streams_dataset_id_length check (octet_length(dataset_id) <= 1024)
);

