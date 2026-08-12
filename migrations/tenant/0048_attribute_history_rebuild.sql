-- ============================================================================
-- 0048: Rebuilt attribute history
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
-- rather than `GEOMETRY(Geometry, 4326)`: 0032 established that no typmod
-- spelling accepts XY and XYZ in one column, which RFC 7946 clause 3.1.1
-- altitudes require.
--
-- `unit_code_id` carries no foreign key to `unit_codes`, and `stream_id` none
-- to `temporal_attribute_streams`. 0023 removed hypertable-to-OLTP foreign
-- keys because TimescaleDB re-adds them per chunk under ShareRowExclusive and
-- deadlocks against concurrent upserts on the parent; 0042 restored the unit
-- guarantee in Rust plus an audit query. There is no cascade from current
-- state onto this table at all: Temporal Evolution deletion (clause 5.6.16)
-- deletes history, then streams, then the temporal entity in one Rust
-- transaction, with an orphan audit as the safety net.
--
-- `chk_attribute_history_value_kind_column` pins `value_kind` to the populated
-- column, which makes a stored value whose declared shape disagrees with where
-- it actually sits unrepresentable rather than something a reader repairs by
-- guessing. It subsumes a range check on `value_kind`.
--
-- Column order is alignment order -- not-null fixed-width descending, then
-- nullable fixed-width, then varlena -- and is the order the binary COPY path
-- stages its fields in.
--
-- No index is built CONCURRENTLY: the table is empty and unreachable at build
-- time, so an ordinary CREATE INDEX inside this transaction is correct and
-- avoids leaving an invalid index behind on failure. No created/modified/
-- deleted-axis index is created yet; those are benchmark-gated.
-- ============================================================================

CREATE TABLE attribute_history (
    recorded_at TIMESTAMPTZ NOT NULL,
    write_id UUID NOT NULL,
    stream_id UUID NOT NULL,
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
    value_string TEXT COMPRESSION lz4,
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

SELECT create_hypertable('attribute_history', by_range('recorded_at', INTERVAL '30 days'));

CREATE INDEX idx_attribute_history_stream_recorded_at
    ON attribute_history (stream_id, recorded_at DESC);

CREATE INDEX idx_attribute_history_stream_observed_at
    ON attribute_history (stream_id, observed_at DESC)
    WHERE observed_at IS NOT NULL;

CREATE INDEX idx_attribute_history_stream_generated_instance
    ON attribute_history (stream_id, generated_instance_uuid)
    WHERE generated_instance_uuid IS NOT NULL;

CREATE INDEX idx_attribute_history_stream_verbatim_instance
    ON attribute_history (stream_id, verbatim_instance_id)
    WHERE verbatim_instance_id IS NOT NULL;

CREATE INDEX idx_attribute_history_value_geo
    ON attribute_history USING GIST (value_geo)
    WHERE value_geo IS NOT NULL;
