-- ============================================================================
-- 0023: Drop hypertable->OLTP foreign keys, replace with statement-level
--       AFTER DELETE triggers, widen chunk_time_interval to 7 days.
-- ============================================================================
-- Background:
--   * `attribute_history.tea_id REFERENCES temporal_entity_attributes(id)` and
--     `scope_history.entity_id REFERENCES entities(id)` cause TimescaleDB to
--     run `ALTER TABLE _hyper_..._chunk ADD CONSTRAINT ... FOREIGN KEY (...)
--     REFERENCES <parent>` every time a fresh chunk is created. The
--     `ADD CONSTRAINT` requires `ShareRowExclusive` on the OLTP parent and
--     races concurrent `RowExclusive` upserts on that same parent, producing
--     `40P01` deadlocks under load.
--   * Cascade semantics that those FKs provided are replaced with statement-
--     level `AFTER DELETE` triggers using `REFERENCING OLD TABLE`, which
--     batch the cascade in a single DELETE per statement (Postgres 10+).
--   * Chunk interval widened from 1 day to 7 days, reducing the rate of
--     chunk-creation DDL contention by ~7x. Existing 1-day chunks are
--     unaffected; only new chunks adopt the 7-day interval.
--   * Compression / columnstore policies (after => INTERVAL '7 days') and
--     continuous aggregates (1h, 1d buckets) are independent of chunk size
--     and need no change.
-- ============================================================================

-- 1. Drop the two FKs that target OLTP tables from hypertables.
ALTER TABLE attribute_history DROP CONSTRAINT IF EXISTS attribute_history_tea_id_fkey;
ALTER TABLE scope_history     DROP CONSTRAINT IF EXISTS scope_history_entity_id_fkey;

-- 2. Replicate ON DELETE CASCADE behaviour with statement-level triggers
--    on the parent tables. `REFERENCING OLD TABLE` exposes the deleted rows
--    as a transition table so a single DELETE handles bulk cascades.
CREATE OR REPLACE FUNCTION trg_tea_cascade_attribute_history()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    DELETE FROM attribute_history ah
    USING old_teas o
    WHERE ah.tea_id = o.id;
    RETURN NULL;
END;
$$;

DROP TRIGGER IF EXISTS tea_cascade_attribute_history ON temporal_entity_attributes;
CREATE TRIGGER tea_cascade_attribute_history
AFTER DELETE ON temporal_entity_attributes
REFERENCING OLD TABLE AS old_teas
FOR EACH STATEMENT EXECUTE FUNCTION trg_tea_cascade_attribute_history();

CREATE OR REPLACE FUNCTION trg_entity_cascade_scope_history()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    DELETE FROM scope_history sh
    USING old_entities o
    WHERE sh.entity_id = o.id;
    RETURN NULL;
END;
$$;

DROP TRIGGER IF EXISTS entity_cascade_scope_history ON entities;
CREATE TRIGGER entity_cascade_scope_history
AFTER DELETE ON entities
REFERENCING OLD TABLE AS old_entities
FOR EACH STATEMENT EXECUTE FUNCTION trg_entity_cascade_scope_history();

-- 3. Widen chunk_time_interval to 7 days for both hypertables.
SELECT set_chunk_time_interval('attribute_history', INTERVAL '7 days');
SELECT set_chunk_time_interval('scope_history',     INTERVAL '7 days');
