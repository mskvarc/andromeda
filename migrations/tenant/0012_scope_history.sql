-- ============================================================================
-- 0012: Scope history
-- ============================================================================
-- Scope history hangs off the temporal catalogue, not off current state, and
-- carries the same shape as attribute history: the operational `recorded_at`
-- partition column, the whole write identity, the four semantic time axes, the
-- split instance identity and the tombstone encoding.
--
-- It has no attribute kind. A scope is not an attribute -- clause 4.5.4 gives
-- it no dataset dimension, no attribute family and no catalogue entry -- so its
-- value vocabulary is its own two-member registry rather than a borrowed
-- subset of the attribute one:
--
--   value_kind: 0 Tombstone (reserved), 1 ScopePath
--
-- Append-only, never renumbered, and this text is the authority.
--
-- A scope deletion is a row carrying `deleted_at` and no path, never an
-- invented sentinel path.
--
-- The permanently stable rendering template for a broker-minted identifier is
-- `urn:ngsi-ld:scope:instance:{uuid}`; a client-supplied identifier is stored
-- exactly as received.
--
-- The path keeps its `ltree` type and its GiST index, which is what scopeQ
-- matching reads (clause 4.18).
--
-- Chunks span seven days, chunks older than thirty days are converted to the
-- columnstore and merged into four-week spans, for the reasons migration 0011
-- gives for attribute history;
-- both intervals are reconciled at startup against the broker's temporal
-- configuration together with the attribute-history ones. Scope history has no
-- stream, so its segment is the temporal entity: a scope query names entities,
-- and one entity's scope rows in a chunk are the rows it reads. The order
-- within a segment is the write-deduplication key of `uq_scope_history_write`.
--
-- Every semantic axis carries a BRIN block summary and no B-tree, again as
-- migration 0011 explains: the entity-led B-trees answer entity-scoped reads,
-- and a broad `observedAt`, `createdAt`, `modifiedAt` or `deletedAt` window
-- that names no entity has the summary as its access path.
-- ============================================================================

CREATE TABLE scope_history (
    recorded_at TIMESTAMPTZ NOT NULL,
    temporal_entity_key BIGINT NOT NULL,
    write_id UUID NOT NULL,
    write_ordinal INTEGER NOT NULL,
    value_kind SMALLINT NOT NULL,
    observed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ,
    modified_at TIMESTAMPTZ,
    deleted_at TIMESTAMPTZ,
    generated_instance_uuid UUID,
    verbatim_instance_id TEXT COMPRESSION lz4 COLLATE "C",
    scope_path ltree COMPRESSION lz4,
    CONSTRAINT uq_scope_history_write UNIQUE (recorded_at, write_id, write_ordinal),
    CONSTRAINT chk_scope_history_write_ordinal CHECK (write_ordinal >= 0),
    CONSTRAINT chk_scope_history_instance_identity
        CHECK (num_nonnulls(generated_instance_uuid, verbatim_instance_id) = 1),
    CONSTRAINT chk_scope_history_value_presence CHECK (
        (value_kind = 0 AND deleted_at IS NOT NULL AND scope_path IS NULL)
        OR
        (value_kind = 1 AND scope_path IS NOT NULL))
);

SELECT create_hypertable('scope_history', by_range('recorded_at', INTERVAL '7 days'));

CREATE INDEX idx_scope_history_entity_recorded_at
    ON scope_history (temporal_entity_key, recorded_at DESC);

CREATE INDEX idx_scope_history_entity_observed_at
    ON scope_history (temporal_entity_key, observed_at DESC)
    WHERE observed_at IS NOT NULL;

CREATE INDEX idx_scope_history_entity_generated_instance
    ON scope_history (temporal_entity_key, generated_instance_uuid)
    WHERE generated_instance_uuid IS NOT NULL;

CREATE INDEX idx_scope_history_entity_verbatim_instance
    ON scope_history (temporal_entity_key, verbatim_instance_id)
    WHERE verbatim_instance_id IS NOT NULL;

CREATE INDEX idx_scope_history_scope_path
    ON scope_history USING GIST (scope_path)
    WHERE scope_path IS NOT NULL;

CREATE INDEX idx_scope_history_observed_at_brin
    ON scope_history USING BRIN (observed_at timestamptz_minmax_multi_ops);

CREATE INDEX idx_scope_history_created_at_brin
    ON scope_history USING BRIN (created_at timestamptz_minmax_multi_ops);

CREATE INDEX idx_scope_history_modified_at_brin
    ON scope_history USING BRIN (modified_at timestamptz_minmax_multi_ops);

CREATE INDEX idx_scope_history_deleted_at_brin
    ON scope_history USING BRIN (deleted_at timestamptz_minmax_multi_ops);

ALTER TABLE scope_history SET (
    timescaledb.enable_columnstore,
    timescaledb.segmentby = 'temporal_entity_key',
    timescaledb.orderby = 'recorded_at DESC, write_id, write_ordinal',
    timescaledb.compress_chunk_time_interval = '28 days'
);

CALL add_columnstore_policy('scope_history', after => INTERVAL '30 days', if_not_exists => true);
