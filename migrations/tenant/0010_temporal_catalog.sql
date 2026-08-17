-- ============================================================================
-- 0045: Temporal catalogue -- temporal entities and attribute streams
-- ============================================================================
-- The temporal side of the broker gets its own catalogue, independent of
-- current state. `temporal_entities` names the Entity a Temporal Evolution
-- belongs to and hands out a narrow BIGINT key; `temporal_attribute_streams`
-- names the stable grain history is recorded at -- one row per (temporal
-- entity, attribute name, datasetId), which is the identity clause 4.5.5
-- defines.
--
-- Neither table references `entities`. Core Entity deletion (clause 5.6.6)
-- leaves temporal state standing and records tombstones, so a cascade from
-- current state would delete history the specification requires to survive.
-- The entity URI is carried here independently, in `temporal_entities.id`.
--
-- `INCLUDE (id)` on the grain constraint makes stream resolution index-only:
-- the write path resolves (entity, attribute, datasetId) to a stream UUID on
-- every append, which is the hottest catalogue read there is. The constraint's
-- leading `temporal_entity_key` also covers the foreign key's cascade check,
-- so no separate index is created for it.
--
-- Stream identifiers are application-generated UUIDv7 from this point on. The
-- column is a plain UUID with no default because the broker mints the value:
-- a database default would hand back an identifier the writer has to read back
-- before it can reference it.
--
-- Column order is alignment order: fixed-width not-null columns first,
-- descending in width, then the varlena columns.
-- ============================================================================

CREATE TABLE temporal_entities (
    entity_key BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL,
    modified_at TIMESTAMPTZ NOT NULL,
    id TEXT COLLATE "C" NOT NULL UNIQUE,
    types TEXT[] NOT NULL,
    CONSTRAINT chk_temporal_entities_types_present CHECK (cardinality(types) > 0)
);

CREATE INDEX idx_temporal_entities_types ON temporal_entities USING GIN (types);

CREATE TABLE temporal_attribute_streams (
    id UUID PRIMARY KEY,
    temporal_entity_key BIGINT NOT NULL REFERENCES temporal_entities(entity_key) ON DELETE CASCADE,
    attribute_name_id INTEGER NOT NULL REFERENCES attribute_names(id),
    dataset_id TEXT COLLATE "C",
    CONSTRAINT uq_temporal_attribute_streams_grain
        UNIQUE NULLS NOT DISTINCT (temporal_entity_key, attribute_name_id, dataset_id) INCLUDE (id)
);
