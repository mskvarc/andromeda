-- ============================================================================
-- 0055: Columnstore on the rebuilt temporal hypertables
-- ============================================================================
-- Enabled last, after the backfill and the freeze, so no policy job can convert
-- a chunk while it is being written and so the freeze ran on rowstore heaps.
--
-- `segmentby` is the stream a row extends -- the column every temporal read
-- filters on -- so each stream compresses and is scanned independently.
--
-- `orderby` is the deduplicating key's column list in the key's own order. That
-- is not cosmetic: TimescaleDB enforces a unique index on a columnar chunk only
-- when the index's columns are covered by segmentby plus orderby, and
-- UNIQUE (recorded_at, write_id, write_ordinal) is the idempotency guarantee
-- the whole replay design rests on.
--
-- The policy delay exceeds the mutation horizon -- the append queue's retry
-- window plus the window in which a back-dated write can still land -- so a
-- chunk is not converted while it is still being written to. It matches the
-- 30-day chunk interval the rebuilt tables were created with: the interval is
-- the density knob, chosen so a stream contributes at least several hundred
-- rows per chunk even at hourly cadence, and operators tune it with
-- set_chunk_time_interval.
--
-- 0008's session-level max_tuples_decompressed_per_dml_transaction is not
-- carried forward: it set a GUC in the migration's own session and had no
-- effect on any later connection.
-- ============================================================================

ALTER TABLE attribute_history SET (
    timescaledb.enable_columnstore,
    timescaledb.segmentby = 'stream_id',
    timescaledb.orderby = 'recorded_at DESC, write_id, write_ordinal'
);

CALL add_columnstore_policy('attribute_history', after => INTERVAL '30 days');

ALTER TABLE scope_history SET (
    timescaledb.enable_columnstore,
    timescaledb.segmentby = 'temporal_entity_key',
    timescaledb.orderby = 'recorded_at DESC, write_id, write_ordinal'
);

CALL add_columnstore_policy('scope_history', after => INTERVAL '30 days');
