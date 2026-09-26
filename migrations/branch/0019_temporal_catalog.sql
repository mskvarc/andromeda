-- ============================================================================
-- 0019: Temporal catalogue -- temporal entities and attribute streams
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
-- `temporal_entities.id` and `temporal_attribute_streams.dataset_id` are both
-- bounded at 1024 UTF-8 octets, the identifier bound (`IDENTIFIER_BYTE_LIMIT`
-- in `andromeda-ngsi-ld-lexicon`) each is parsed under, which keeps every
-- btree entry that carries one well inside the 2704 bytes a btree admits.
--
-- `temporal_entities.id` is `COLLATE "C"`, so an anchored clause 4.9
-- `idPattern` compiles to a range scan of `uq_temporal_entities_id`.
-- `idx_temporal_entities_id_trgm` serves the unanchored patterns the temporal
-- entity selection emits as `~`, exactly as `idx_entities_id_trgm` serves
-- them against current state. Its `entity_key` identity declares `CACHE 32`
-- like `entities`: temporal entity creation is batched and concurrent, and a
-- surrogate that is never rendered loses nothing to the gaps a cached sequence
-- leaves after a restart.
--
-- `INCLUDE (stream_key)` on the grain constraint makes stream resolution
-- index-only: the write path resolves (entity, attribute, datasetId) to a
-- stream key on every append, which is the hottest catalogue read there is.
-- The constraint's leading `temporal_entity_key` also covers the foreign key's
-- cascade check, so no separate index is created for it.
--
-- `stream_key` is a database-generated BIGINT identity. The key repeats on
-- every `attribute_history` row and leads every stream-led history index, so
-- its width is paid once per row and once per index entry; eight bytes is the
-- narrowest key that cannot run out, and keys minted close together sort close
-- together on the physical page instead of scattering across it.
-- The key is never rendered to a client, so nothing outside the database has
-- to be able to name a stream before it exists; the write path inserts the
-- grain and reads the key back in the same transaction. `CACHE 32` lets a
-- write that creates several streams take its keys from one sequence fetch.
--
-- `type_ids` is an `INTEGER[]` of `entity_type_names` keys (migration 0008),
-- the same interning `entities.type_ids` uses: a temporal query's type
-- selection (clause 4.17) is a containment test on the keys, served by the
-- GIN index below, and the IRIs are read back through the dictionary when a
-- Temporal Evolution is rendered. The keys carry no foreign key -- an array
-- element cannot be one -- and are minted in the transaction that writes them.
--
-- Column order is alignment order: fixed-width not-null columns first,
-- descending in width, then the varlena columns, LZ4 on each varlena column
-- that is not an identifier looked up by equality.
--
-- Branch relation: this file is in `migrations/branch/`, so it creates the live
-- relations in `public` as part of the tenant migration set and creates and
-- upgrades the same relations inside every Snapshot schema under the branch
-- ledger. Dictionary, unit-code and function references resolve to `public`
-- through the search path; every other reference resolves to the branch.
-- ============================================================================

CREATE TABLE temporal_entities (
    entity_key BIGINT GENERATED ALWAYS AS IDENTITY (CACHE 32),
    created_at TIMESTAMPTZ NOT NULL,
    modified_at TIMESTAMPTZ NOT NULL,
    id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    type_ids INTEGER[] NOT NULL,
    CONSTRAINT pk_temporal_entities PRIMARY KEY (entity_key),
    CONSTRAINT uq_temporal_entities_id UNIQUE (id),
    CONSTRAINT chk_temporal_entities_types_present CHECK (cardinality(type_ids) > 0),
    CONSTRAINT chk_temporal_entities_id_length CHECK (octet_length(id) <= 1024)
);

CREATE INDEX idx_temporal_entities_type_ids ON temporal_entities USING GIN (type_ids);

CREATE INDEX idx_temporal_entities_id_trgm ON temporal_entities USING GIN (id gin_trgm_ops);

CREATE TABLE temporal_attribute_streams (
    stream_key BIGINT GENERATED ALWAYS AS IDENTITY (CACHE 32),
    temporal_entity_key BIGINT NOT NULL,
    attribute_name_id INTEGER NOT NULL,
    dataset_id TEXT COMPRESSION lz4 COLLATE "C",
    CONSTRAINT pk_temporal_attribute_streams PRIMARY KEY (stream_key),
    CONSTRAINT uq_temporal_attribute_streams_grain
        UNIQUE NULLS NOT DISTINCT (temporal_entity_key, attribute_name_id, dataset_id) INCLUDE (stream_key),
    CONSTRAINT fk_temporal_attribute_streams_entity
        FOREIGN KEY (temporal_entity_key)
        REFERENCES temporal_entities(entity_key) ON DELETE CASCADE,
    CONSTRAINT fk_temporal_attribute_streams_attribute_name
        FOREIGN KEY (attribute_name_id)
        REFERENCES attribute_names(id),
    CONSTRAINT chk_temporal_attribute_streams_dataset_id_length CHECK (octet_length(dataset_id) <= 1024)
);
