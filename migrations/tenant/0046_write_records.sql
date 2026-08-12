-- ============================================================================
-- 0046: Write records -- the write's own idempotency record
-- ============================================================================
-- `write_records` replaces `temporal_history_sink_receipts`. It is not a sink
-- receipt: it is the record of one atomic broker write, and it persists that
-- write's `recorded_at` so a replay reuses the original instant instead of
-- stamping a new one. The rebuilt history hypertable is deduplicated by
-- (recorded_at, write_id, write_ordinal) -- every unique index on a hypertable
-- must contain the partition column -- so a replay that re-stamped ingestion
-- time would never collide with the rows it duplicates. Reusing the persisted
-- instant is what makes the collision happen.
--
-- A record lives for as long as any effect it protects exists, and is released
-- by Temporal Evolution deletion (clause 5.6.16) or by the operator-configured
-- horizon sweep, which is what `idx_write_records_recorded_at` serves.
--
-- `accepted_at` is the instant the broker accepted the write, kept distinct
-- from `recorded_at` because a replayed write is accepted again at a later
-- moment while recording at the original one.
--
-- The validation idiom is the one 0042 established: casting a sentence to
-- SMALLINT raises only when the offending set is non-empty, and the sentence
-- must reference a column so the planner cannot constant-fold it into a
-- failure on an empty table.
-- ============================================================================

CREATE TABLE write_records (
    recorded_at TIMESTAMPTZ NOT NULL,
    accepted_at TIMESTAMPTZ NOT NULL,
    write_id UUID PRIMARY KEY
);

CREATE INDEX idx_write_records_recorded_at ON write_records (recorded_at);

INSERT INTO write_records (recorded_at, accepted_at, write_id)
SELECT r.recorded_at, r.accepted_at, r.write_id
FROM temporal_history_sink_receipts AS r;

SELECT CAST('write_records backfill copied ' || rebuilt.n::text || ' rows from ' || legacy.n::text AS SMALLINT)
FROM (SELECT count(*) AS n FROM temporal_history_sink_receipts) AS legacy,
     (SELECT count(*) AS n FROM write_records) AS rebuilt
WHERE legacy.n <> rebuilt.n;
