-- ============================================================================
-- 0016: Write idempotency record
-- ============================================================================
-- `write_records` is the record of one atomic broker write, and persists
-- that write's `recorded_at` so a replay reuses the original instant instead
-- of stamping a new one. `attribute_history` is deduplicated by
-- (recorded_at, write_id, write_ordinal) -- every unique index on a
-- hypertable must contain the partition column -- so a replay that
-- re-stamped ingestion time would never collide with the rows it
-- duplicates. Reusing the persisted instant is what makes the collision
-- happen.
--
-- It is tenant-global rather than branch state: a write identity is minted
-- once per tenant, and a Snapshot branch is populated by capture and clone,
-- never by a replayable broker write.
--
-- A row lives for as long as any effect it protects exists, and is released
-- by Temporal Evolution deletion (clause 5.6.16) or by the operator-configured
-- horizon sweep, which is what `idx_write_records_recorded_at` serves.
--
-- `accepted_at` is the instant the broker accepted the write, kept distinct
-- from `recorded_at` because a replayed write is accepted again at a later
-- moment while recording at the original one.
-- ============================================================================

CREATE TABLE write_records (
    recorded_at TIMESTAMPTZ NOT NULL,
    accepted_at TIMESTAMPTZ NOT NULL,
    write_id UUID PRIMARY KEY
);

CREATE INDEX idx_write_records_recorded_at ON write_records (recorded_at);
