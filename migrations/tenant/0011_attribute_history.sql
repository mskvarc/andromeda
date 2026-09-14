-- ============================================================================
-- 0011: Attribute history
-- ============================================================================
-- One row per recorded attribute instance, on four semantic time axes plus one
-- operational one.
--
-- `recorded_at` is the partition column and the write clock. It is never
-- rendered as an NGSI-LD Temporal Property: `observed_at`, `created_at`,
-- `modified_at` and `deleted_at` are the four the specification admits as a
-- timeproperty (clause 5.2.21), each nullable and each written only from what
-- was actually observed. A time filter therefore selects exactly the requested
-- column, with no fallback: a row carrying no observedAt matches no observedAt
-- filter rather than inventing a match from the write clock (clause 4.11).
--
-- `attribute_kind` and `value_kind` are compact migration-owned codes carried
-- on every row. A historical row's kind must not be reinterpretable through a
-- mutable catalogue: an attribute whose type changes does not retroactively
-- change what was already recorded. `attribute_kind` is present on tombstones
-- too, because synthesising `urn:ngsi-ld:null` for a deletion needs the member
-- shape the family dictates -- a value for a Property, an object for a
-- Relationship, a language map for a LanguageProperty (clauses 4.5.7, 4.5.8).
--
--   attribute_kind: 1 Property, 2 Relationship, 3 GeoProperty,
--                   4 VocabProperty, 5 LanguageProperty, 6 JsonProperty,
--                   7 ListProperty, 8 ListRelationship
--   value_kind:     0 Tombstone (reserved), 1 Number, 2 String, 3 Boolean,
--                   4 Geometry, 5 Object, 6 DateTime, 7 Uri
--
-- Both registries are append-only and never renumbered. This text is the
-- authority; the Rust enums are checked against it.
--
-- A tombstone carries `deleted_at` and no typed value at all. The null
-- representation is synthesised at render time and is never stored as though a
-- client had supplied it as a URI.
--
-- Instance identity is split. A broker-minted identifier is stored as the UUID
-- it is and rendered through one permanently stable template,
-- `urn:ngsi-ld:attribute:instance:{uuid}`; a client-supplied one is stored
-- exactly as received, because clause 4.5.7 hands the identifier back to the
-- client. Exactly one of the two columns is populated.
--
-- `value_geo` is typmod-free `geometry` with explicit SRID and Zm-flag checks
-- rather than `GEOMETRY(Geometry, 4326)`: no typmod spelling accepts XY and
-- XYZ in one column, which RFC 7946 clause 3.1.1 altitudes require.
--
-- `unit_code_id` carries no foreign key to `unit_codes`, and `stream_key` none
-- to `temporal_attribute_streams`. Hypertable-to-OLTP foreign keys are absent
-- because TimescaleDB re-adds them per chunk under ShareRowExclusive and
-- deadlocks against concurrent upserts on the parent; the unit guarantee is
-- kept in Rust plus an audit query. There is no cascade from current
-- state onto this table at all: Temporal Evolution deletion (clause 5.6.16)
-- deletes history, then streams, then the temporal entity in one Rust
-- transaction, with an orphan audit as the safety net.
--
-- `chk_attribute_history_value_kind_column` pins `value_kind` to the populated
-- column, which makes a stored value whose declared shape disagrees with where
-- it actually sits unrepresentable rather than something a reader repairs by
-- guessing. It subsumes a range check on `value_kind`.
--
-- Every text column here is `COLLATE "C"`, including `value_string`. Clause 4.9
-- orders string values with `<`, `>`, `<=` and `>=`, and clause 4.5.19's `min`
-- and `max` over a string Property are answered by aggregating this column
-- directly; the same comparison is answered from
-- `current_attribute_values.value_string` for current state. A locale collation
-- would order the two columns by whatever locale the cluster was initialised
-- with, so temporal aggregation and current-state `q` could disagree on the same
-- data. Byte-wise order is deterministic, is identical on every cluster, and is
-- conformant: clause 4.9 defers string comparison to RFC 8259 clause 8.3 and
-- only *recommends* Unicode Collation Algorithm ordering, so a deliberately
-- code-point-oriented order is a choice the specification admits.
-- `docs/text-collation-invariant.md` states the invariant for the whole schema.
--
-- `stream_key` is the eight-byte key `temporal_attribute_streams` generates
-- (migration 0010). It is the column every stream-led index below leads with
-- and the column the columnstore segments by, so its width is paid on every
-- row and every index entry; the BIGINT halves what the earlier UUID cost.
--
-- Column order is alignment order -- not-null fixed-width descending, then
-- nullable fixed-width, then varlena -- and is the order the binary COPY path
-- stages its fields in.
--
-- No index is built CONCURRENTLY: the table is empty and unreachable at build
-- time, so an ordinary CREATE INDEX inside this transaction is correct and
-- avoids leaving an invalid index behind on failure. TimescaleDB refuses
-- concurrent index creation on a hypertable outright, so this is the only
-- spelling available here in any case.
--
-- Chunks span seven days. The stream-led B-trees are per chunk, and only the
-- chunks still receiving rows have index pages that must stay in memory; a
-- shorter chunk keeps that active set bounded regardless of how far back the
-- table reaches, while `compress_chunk_time_interval` below merges chunks back
-- into four-week spans once they are converted, so the compressed tail is not
-- fragmented by the write-side interval. The merge span is 28 rather than 30
-- days because TimescaleDB requires it to be a whole multiple of the chunk
-- interval. The chunk interval and the policy lag are defaults the broker
-- reconciles at startup against its temporal configuration, so an operator
-- change does not need a migration; an operator who changes the chunk interval
-- to a value that does not divide 28 days must expect the merge to be refused
-- with a warning rather than applied.
--
-- Columnstore. Chunks older than thirty days are converted to the columnstore
-- by the policy added at the end of this migration; thirty days is the horizon
-- within which history is expected to be mutated in place (instance
-- modification and deletion, clauses 5.6.14 and 5.6.15, and the verbatim
-- instanceId dedup that precedes a replacing write) and past which it is read
-- as an immutable archive. Rows are segmented by `stream_key`: a temporal query
-- names streams, so a segment holds exactly the rows one stream contributed to
-- the chunk and a stream-led scan touches only its own segments. Within a
-- segment rows are ordered by `recorded_at DESC, write_id, write_ordinal` --
-- the write-deduplication key of `uq_attribute_history_write` -- so the newest
-- observation of a stream leads its segment and a batch's rows stay contiguous.
-- ============================================================================

CREATE TABLE attribute_history (
    recorded_at TIMESTAMPTZ NOT NULL,
    write_id UUID NOT NULL,
    stream_key BIGINT NOT NULL,
    write_ordinal INTEGER NOT NULL,
    attribute_kind SMALLINT NOT NULL,
    value_kind SMALLINT NOT NULL,
    observed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ,
    modified_at TIMESTAMPTZ,
    deleted_at TIMESTAMPTZ,
    value_datetime TIMESTAMPTZ,
    generated_instance_uuid UUID,
    unit_code_id SMALLINT,
    value_bool BOOLEAN,
    verbatim_instance_id TEXT COMPRESSION lz4 COLLATE "C",
    value_number NUMERIC COMPRESSION lz4,
    value_string TEXT COMPRESSION lz4 COLLATE "C",
    value_geo geometry COMPRESSION lz4,
    value_object JSONB COMPRESSION lz4,
    value_uri TEXT COMPRESSION lz4 COLLATE "C",
    metadata JSONB COMPRESSION lz4,
    CONSTRAINT uq_attribute_history_write UNIQUE (recorded_at, write_id, write_ordinal),
    CONSTRAINT chk_attribute_history_write_ordinal CHECK (write_ordinal >= 0),
    CONSTRAINT chk_attribute_history_attribute_kind CHECK (attribute_kind BETWEEN 1 AND 8),
    CONSTRAINT chk_attribute_history_instance_identity
        CHECK (num_nonnulls(generated_instance_uuid, verbatim_instance_id) = 1),
    CONSTRAINT chk_attribute_history_value_presence CHECK (
        (value_kind = 0
            AND deleted_at IS NOT NULL
            AND num_nonnulls(value_number, value_string, value_bool, value_geo,
                             value_object, value_datetime, value_uri) = 0)
        OR
        (value_kind <> 0
            AND num_nonnulls(value_number, value_string, value_bool, value_geo,
                             value_object, value_datetime, value_uri) = 1)),
    CONSTRAINT chk_attribute_history_value_kind_column CHECK (
        value_kind = 0
        OR (value_kind = 1 AND value_number IS NOT NULL)
        OR (value_kind = 2 AND value_string IS NOT NULL)
        OR (value_kind = 3 AND value_bool IS NOT NULL)
        OR (value_kind = 4 AND value_geo IS NOT NULL)
        OR (value_kind = 5 AND value_object IS NOT NULL)
        OR (value_kind = 6 AND value_datetime IS NOT NULL)
        OR (value_kind = 7 AND value_uri IS NOT NULL)),
    CONSTRAINT chk_attribute_history_value_geo_valid
        CHECK (value_geo IS NULL OR ST_IsValid(value_geo)),
    CONSTRAINT chk_attribute_history_value_geo_srid
        CHECK (value_geo IS NULL OR ST_SRID(value_geo) = 4326),
    CONSTRAINT chk_attribute_history_value_geo_zm
        CHECK (value_geo IS NULL OR ST_Zmflag(value_geo) IN (0, 2))
);

SELECT create_hypertable('attribute_history', by_range('recorded_at', INTERVAL '7 days'));

CREATE INDEX idx_attribute_history_stream_recorded_at
    ON attribute_history (stream_key, recorded_at DESC);

CREATE INDEX idx_attribute_history_stream_observed_at
    ON attribute_history (stream_key, observed_at DESC)
    WHERE observed_at IS NOT NULL;

CREATE INDEX idx_attribute_history_stream_generated_instance
    ON attribute_history (stream_key, generated_instance_uuid)
    WHERE generated_instance_uuid IS NOT NULL;

CREATE INDEX idx_attribute_history_stream_verbatim_instance
    ON attribute_history (stream_key, verbatim_instance_id)
    WHERE verbatim_instance_id IS NOT NULL;

CREATE INDEX idx_attribute_history_value_geo
    ON attribute_history USING GIST (value_geo)
    WHERE value_geo IS NOT NULL;

-- ----------------------------------------------------------------------------
-- Semantic-time summary over the whole hypertable
-- ----------------------------------------------------------------------------
-- `recorded_at` is the partition column, so a window on it is answered by chunk
-- exclusion alone. `observed_at` is not, and the two indexes above lead with
-- `stream_key`, so a temporal query that bounds `observedAt` without naming a
-- stream -- which clause 5.7.2 permits, since `timerel` is independent of the
-- entity selector -- has no access path but a full scan of every chunk.
--
-- The measured shape of that gap and of this index is
-- `docs/proposals/database-schema-redesign-benchmarks.md` section 8: over four
-- million rows in thirteen chunks the summary cut a broad seven-day window from
-- 81.5 ms to 15.6 ms and its buffer traffic from 20,240 to 7,208, for 632 kB --
-- 0.02% of the hypertable -- with no plan regression on the stream-led path,
-- which keeps its B-tree. This is an addition to those indexes, never a
-- replacement. The accept rests on a per-chunk `observed_at` correlation of
-- 0.99 measured against synthetic data; below roughly 0.9 a BRIN summary
-- degrades into a sequential scan carrying recheck cost on top, so production
-- correlation and the real late-observation tail have to be measured before
-- this index is treated as permanent, and re-measured after any change to the
-- chunk interval.
--
-- `timestamptz_minmax_multi_ops` rather than the default single-range
-- `timestamptz_minmax_ops`: a late observation arriving a year after the fact
-- stretches a single per-block range across that year and makes the summary
-- match every query, whereas a multi-range summary keeps the outlier in its own
-- range and still excludes the block. The measured one-year-late window is
-- exactly that case.
--
-- Deliberately not partial. The equivalent `WHERE observed_at IS NOT NULL`
-- predicate would buy nothing -- BRIN summarises nulls per block rather than
-- indexing them individually, so the null rows cost no entries -- and it would
-- cost two things: the planner needs the predicate proven to use a partial
-- index, and TimescaleDB's columnstore sparse-index builder skips partial
-- indexes when deciding which columns get a minmax summary on a compressed
-- chunk, which is precisely where this summary matters most.
CREATE INDEX idx_attribute_history_observed_at_brin
    ON attribute_history USING BRIN (observed_at timestamptz_minmax_multi_ops);

-- The three remaining semantic axes take the same summary for the same reason:
-- clause 5.2.21 lets a temporal query select `createdAt`, `modifiedAt` or
-- `deletedAt` as its timeproperty, and none of them is the partition column or
-- leads any B-tree. Each is a nullable axis written only from what was
-- observed, so the block summary is the whole access path a broad window on it
-- has, at BRIN's per-block cost rather than a per-row B-tree's. Not partial,
-- for the same two reasons as the `observed_at` summary.
CREATE INDEX idx_attribute_history_created_at_brin
    ON attribute_history USING BRIN (created_at timestamptz_minmax_multi_ops);

CREATE INDEX idx_attribute_history_modified_at_brin
    ON attribute_history USING BRIN (modified_at timestamptz_minmax_multi_ops);

CREATE INDEX idx_attribute_history_deleted_at_brin
    ON attribute_history USING BRIN (deleted_at timestamptz_minmax_multi_ops);

-- ----------------------------------------------------------------------------
-- Columnstore
-- ----------------------------------------------------------------------------
-- The rationale is in the header. Configured after the indexes so that every
-- index exists before the first chunk could ever be converted; the policy's
-- lag is what the broker's temporal configuration overrides at startup.
ALTER TABLE attribute_history SET (
    timescaledb.enable_columnstore,
    timescaledb.segmentby = 'stream_key',
    timescaledb.orderby = 'recorded_at DESC, write_id, write_ordinal',
    timescaledb.compress_chunk_time_interval = '28 days'
);

CALL add_columnstore_policy('attribute_history', after => INTERVAL '30 days', if_not_exists => true);

-- ----------------------------------------------------------------------------
-- Verbatim instanceId uniqueness
-- ----------------------------------------------------------------------------
-- Clause 4.5.7 makes a client-supplied `instanceId` unique to one attribute
-- instance of one attribute, on whichever `datasetId` stream carries it. The
-- natural expression -- a unique index on `attribute_history` over
-- `(temporal_entity_key, attribute_name_id, verbatim_instance_id)` -- cannot be
-- built: `attribute_history` is a hypertable partitioned on `recorded_at`, and
-- TimescaleDB refuses a unique index that omits the partitioning column, since
-- per-chunk indexes cannot otherwise see a duplicate landing in a different
-- chunk. Including `recorded_at` in the index would not fix that either --
-- every write stamps its own wall-clock instant, so two rows claiming the same
-- instanceId at different moments would still both pass it.
--
-- The invariant is therefore carried on an ordinary table instead, keyed by
-- entity and attribute rather than by stream, because clause 4.5.7 does not
-- scope `instanceId` to one `datasetId`: two sibling streams of the same
-- attribute claiming the same instanceId is exactly the collision this table
-- exists to refuse, not two independent identities that happen to collide.
-- `idx_attribute_history_stream_verbatim_instance` above remains a lookup
-- index only and carries no uniqueness of its own.
--
-- A row here is staked when a history row carrying a verbatim instanceId is
-- written and freed when that row is removed -- by the dedup delete that
-- precedes a replacing write, or by instance DELETE (clause 5.6.15). Neither
-- path touches this table for a broker-minted identifier, which is unique by
-- construction and never claims a row.
CREATE TABLE attribute_history_instance_claims (
    temporal_entity_key BIGINT NOT NULL,
    attribute_name_id INTEGER NOT NULL REFERENCES attribute_names(id),
    verbatim_instance_id TEXT COLLATE "C" NOT NULL,
    CONSTRAINT pk_attribute_history_instance_claims
        PRIMARY KEY (temporal_entity_key, attribute_name_id, verbatim_instance_id),
    CONSTRAINT fk_attribute_history_instance_claims_entity
        FOREIGN KEY (temporal_entity_key)
        REFERENCES temporal_entities(entity_key) ON DELETE CASCADE
);
