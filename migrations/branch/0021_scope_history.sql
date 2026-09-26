-- ============================================================================
-- 0021: Scope history
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.18, 4.19, 5.2.21
--
-- The recorded Scopes of each temporal entity, in the shape of attribute
-- history: the `recorded_at` write clock, the write identity, the four
-- semantic time axes, the two-column instance identity and the tombstone. The
-- live copy is a hypertable partitioned on `recorded_at`; a Snapshot's copy
-- stays a plain table.
--
-- Codes:
--
--   value_kind: 0 tombstone, 1 scope-path
--
-- A Scope is not an attribute: clause 4.18 gives it no dataset dimension, no
-- attribute family and no catalog entry, so there is no attribute kind and
-- its value kinds are its own. A Scope deletion is a row with `deleted_at` and
-- no path, never an invented sentinel path.
--
-- A broker-minted instance identifier is rendered through the permanently
-- stable template `urn:ngsi-ld:scope:instance:{uuid}`; a client-supplied one
-- is kept exactly as received.
--
-- Each semantic axis has a BRIN summary and no B-tree: the entity-led B-trees
-- answer entity-scoped reads, and a broad `observedAt`, `createdAt`,
-- `modifiedAt` or `deletedAt` window that names no entity reads the summary.
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
    generated_instance_id UUID,
    verbatim_instance_id TEXT COMPRESSION lz4 COLLATE "C",
    scope_path ltree COMPRESSION lz4,
    CONSTRAINT uq_scope_history_write_identity UNIQUE (recorded_at, write_id, write_ordinal),
    CONSTRAINT chk_scope_history_write_ordinal CHECK (write_ordinal >= 0),
    CONSTRAINT chk_scope_history_instance_identity
        CHECK (num_nonnulls(generated_instance_id, verbatim_instance_id) = 1),
    CONSTRAINT chk_scope_history_verbatim_instance_id_length
        CHECK (octet_length(verbatim_instance_id) <= 1024),
    CONSTRAINT chk_scope_history_value_presence CHECK (
        (value_kind = 0 AND deleted_at IS NOT NULL AND scope_path IS NULL)
        OR
        (value_kind = 1 AND scope_path IS NOT NULL))
);

CREATE INDEX idx_scope_history_entity_recorded_at
    ON scope_history (temporal_entity_key, recorded_at DESC);

CREATE INDEX idx_scope_history_entity_observed_at
    ON scope_history (temporal_entity_key, observed_at DESC)
    WHERE observed_at IS NOT NULL;

CREATE INDEX idx_scope_history_entity_generated_instance
    ON scope_history (temporal_entity_key, generated_instance_id)
    WHERE generated_instance_id IS NOT NULL;

CREATE INDEX idx_scope_history_entity_verbatim_instance
    ON scope_history (temporal_entity_key, verbatim_instance_id)
    WHERE verbatim_instance_id IS NOT NULL;

-- `scopeQ` matching over recorded Scopes (clause 4.19).
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
