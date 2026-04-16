-- ============================================================================
-- 016: Continuous Aggregates -- Hot Aggregation Buckets (TSL-gated)
-- ============================================================================
-- Pre-aggregated 1-hour and 1-day numeric buckets over attribute_history,
-- refreshed by TimescaleDB continuous aggregate policies. The temporal query
-- builder routes aggregation requests whose period is PT1H or PT1D and whose
-- methods fall within {avg, min, max, sum, totalCount} to these views;
-- other periods or methods continue to hit raw attribute_history.
--
-- Continuous aggregates are a TimescaleDB TSL feature. The migration probes
-- pg_proc for add_continuous_aggregate_policy (present only when TSL is
-- available) and branches accordingly -- mirroring the columnstore detection
-- in 0008_attribute_history.sql.
-- ============================================================================

DO $$
DECLARE
    has_caggs BOOLEAN;
BEGIN
    SELECT EXISTS(
        SELECT 1 FROM pg_proc WHERE proname = 'add_continuous_aggregate_policy'
    ) INTO has_caggs;

    IF has_caggs THEN
        RAISE NOTICE 'TSL available -- creating continuous aggregates attr_agg_1h, attr_agg_1d';

        EXECUTE $cagg$
            CREATE MATERIALIZED VIEW attr_agg_1h
                WITH (timescaledb.continuous) AS
                SELECT tea_id,
                       time_bucket(INTERVAL '1 hour', time) AS bucket,
                       AVG(value_number)::DOUBLE PRECISION AS avg_num,
                       MIN(value_number) AS min_num,
                       MAX(value_number) AS max_num,
                       SUM(value_number) AS sum_num,
                       COUNT(*)          AS total_count
                FROM attribute_history
                WHERE value_number IS NOT NULL
                GROUP BY tea_id, bucket
                WITH NO DATA
        $cagg$;

        PERFORM add_continuous_aggregate_policy(
            'attr_agg_1h',
            start_offset      => INTERVAL '2 days',
            end_offset        => INTERVAL '1 hour',
            schedule_interval => INTERVAL '30 minutes'
        );

        EXECUTE $cagg$
            CREATE MATERIALIZED VIEW attr_agg_1d
                WITH (timescaledb.continuous) AS
                SELECT tea_id,
                       time_bucket(INTERVAL '1 day', time) AS bucket,
                       AVG(value_number)::DOUBLE PRECISION AS avg_num,
                       MIN(value_number) AS min_num,
                       MAX(value_number) AS max_num,
                       SUM(value_number) AS sum_num,
                       COUNT(*)          AS total_count
                FROM attribute_history
                WHERE value_number IS NOT NULL
                GROUP BY tea_id, bucket
                WITH NO DATA
        $cagg$;

        PERFORM add_continuous_aggregate_policy(
            'attr_agg_1d',
            start_offset      => INTERVAL '14 days',
            end_offset        => INTERVAL '1 day',
            schedule_interval => INTERVAL '6 hours'
        );
    ELSE
        RAISE NOTICE 'TSL not available -- skipping continuous aggregates; aggregation queries will use raw attribute_history';
    END IF;
END
$$;
