-- ============================================================================
-- 0023: Temporal hypercore columnstore
-- ============================================================================
-- Hypercore columnstore compression for the two temporal hypertables,
-- attribute_history (0011) and scope_history (0012). It is kept out of those
-- table-creation migrations so both stay edition-neutral -- pure structure every
-- TimescaleDB build accepts -- and one migration owns the single
-- edition-conditional feature the schema uses.
--
-- Editions. enable_columnstore and add_columnstore_policy are TSL (Timescale
-- License) capabilities. The published image ships in two editions: the TSL
-- build has them, the Apache-2.0 core does not. The block is therefore gated on
-- add_columnstore_policy being registered in pg_proc: on the TSL edition the
-- hypertables gain the columnstore; on the Apache-2.0 core the probe is false
-- and they stay plain hypertables, read uncompressed. The runtime storage-policy
-- reconciler probes the same fact, so it never calls a procedure the edition
-- lacks. There is deliberately no legacy timescaledb.compress fallback branch:
-- that spelling is also TSL-only, so it would help no Apache install.
--
-- This runs after every table-creation migration, so both hypertables and all
-- their indexes already exist; no chunk has been written yet in a fresh
-- migration run, so nothing is converted while it runs.
--
-- Conversion horizon. Chunks older than thirty days are converted: thirty days
-- is the horizon within which history is expected to be mutated in place
-- (instance modification and deletion, clauses 5.6.14 and 5.6.15, and the
-- verbatim instanceId dedup that precedes a replacing write) and past which it
-- is read as an immutable archive. compress_chunk_time_interval merges converted
-- chunks back into four-week spans so the compressed tail is not fragmented by
-- the seven-day write-side interval; the merge span is 28 rather than 30 days
-- because TimescaleDB requires it to be a whole multiple of the chunk interval.
-- Both the chunk interval and the policy lag are defaults the broker reconciles
-- at startup against its temporal configuration; an operator who moves the chunk
-- interval to a value that does not divide 28 days must expect the merge to be
-- refused with a warning rather than applied.
--
-- Segmentation. attribute_history is segmented by stream_key: a temporal query
-- names streams, so a segment holds exactly the rows one stream contributed to
-- the chunk and a stream-led scan touches only its own segments. scope_history
-- has no stream, so its segment is the temporal entity -- a scope query names
-- entities, and one entity's scope rows in a chunk are the rows it reads. Within
-- a segment rows are ordered by recorded_at DESC, write_id, write_ordinal, the
-- write-deduplication key of each hypertable's uq_*_write constraint, so the
-- newest observation leads its segment and a batch's rows stay contiguous.
-- ============================================================================

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_proc WHERE proname = 'add_columnstore_policy') THEN
        ALTER TABLE attribute_history SET (
            timescaledb.enable_columnstore,
            timescaledb.segmentby = 'stream_key',
            timescaledb.orderby = 'recorded_at DESC, write_id, write_ordinal',
            timescaledb.compress_chunk_time_interval = '28 days'
        );
        CALL add_columnstore_policy('attribute_history', after => INTERVAL '30 days', if_not_exists => true);

        ALTER TABLE scope_history SET (
            timescaledb.enable_columnstore,
            timescaledb.segmentby = 'temporal_entity_key',
            timescaledb.orderby = 'recorded_at DESC, write_id, write_ordinal',
            timescaledb.compress_chunk_time_interval = '28 days'
        );
        CALL add_columnstore_policy('scope_history', after => INTERVAL '30 days', if_not_exists => true);
    END IF;
END $$;
