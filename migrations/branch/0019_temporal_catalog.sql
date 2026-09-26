-- ============================================================================
-- 0019: Temporal catalog
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

CREATE TABLE temporal_entities (
    temporal_entity_key BIGINT GENERATED ALWAYS AS IDENTITY (CACHE 32),
    created_at TIMESTAMPTZ NOT NULL,
    modified_at TIMESTAMPTZ NOT NULL,
    id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    -- `entity_type_names` keys.
    entity_type_name_keys INTEGER[] COMPRESSION lz4 NOT NULL,
    CONSTRAINT pk_temporal_entities PRIMARY KEY (temporal_entity_key),
    CONSTRAINT uq_temporal_entities_id UNIQUE (id),
    CONSTRAINT chk_temporal_entities_entity_type_name_keys_present CHECK (cardinality(entity_type_name_keys) > 0),
    CONSTRAINT chk_temporal_entities_id_length CHECK (octet_length(id) <= 1024)
);

CREATE INDEX idx_temporal_entities_entity_type_name_keys ON temporal_entities USING GIN (entity_type_name_keys);

CREATE INDEX idx_temporal_entities_id_trgm ON temporal_entities USING GIN (id gin_trgm_ops);

CREATE TABLE temporal_attribute_streams (
    -- Minted when the write path first inserts the grain and read back in the
    -- same transaction; never rendered.
    stream_key BIGINT GENERATED ALWAYS AS IDENTITY (CACHE 32),
    temporal_entity_key BIGINT NOT NULL,
    attribute_name_key INTEGER NOT NULL,
    dataset_id TEXT COMPRESSION lz4 COLLATE "C",
    CONSTRAINT pk_temporal_attribute_streams PRIMARY KEY (stream_key),
    -- `INCLUDE (stream_key)` makes stream resolution index-only: every append
    -- resolves its grain to a stream key, the hottest catalog read there is.
    CONSTRAINT uq_temporal_attribute_streams_grain
        UNIQUE NULLS NOT DISTINCT (temporal_entity_key, attribute_name_key, dataset_id) INCLUDE (stream_key),
    CONSTRAINT fk_temporal_attribute_streams_temporal_entity
        FOREIGN KEY (temporal_entity_key)
        REFERENCES temporal_entities(temporal_entity_key) ON DELETE CASCADE,
    CONSTRAINT chk_temporal_attribute_streams_dataset_id_length CHECK (octet_length(dataset_id) <= 1024)
);
