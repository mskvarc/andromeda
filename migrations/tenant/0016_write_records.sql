-- ============================================================================
-- 0016: Write records
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clause 5.6.16
--
-- One row per atomic broker write, holding the instant the write was recorded
-- at, so a replay of the write reuses that instant instead of stamping a new
-- one. History rows are deduplicated on the recorded instant together with the
-- write identity, so a replay that stamped a fresh instant would never collide
-- with the rows it duplicates.
--
-- A row lives as long as any effect it protects, and is released by Temporal
-- Evolution deletion (clause 5.6.16) or by the operator's retention horizon,
-- whose sweep ranges over `recorded_at`.
-- ============================================================================

CREATE TABLE write_records (
    recorded_at TIMESTAMPTZ NOT NULL,
    -- When the broker accepted the write. A replay is accepted again, later,
    -- while it keeps recording at the original instant.
    accepted_at TIMESTAMPTZ NOT NULL,
    write_id UUID NOT NULL,
    CONSTRAINT pk_write_records PRIMARY KEY (write_id)
);

CREATE INDEX idx_write_records_recorded_at ON write_records (recorded_at);
