-- ============================================================================
-- 0052: Backfill the rebuilt scope history
-- ============================================================================
-- Legacy scope rows carry no write identity of any kind -- 0030 added one to
-- attribute history only -- so every migrated row gets a synthesised identity
-- on the same content-digest rule the attribute backfill uses, with ordinal 0.
--
-- They also carry no observation timestamp, no system timestamps and no
-- deletion encoding: scope removal was expressed as absence rather than as a
-- row. Every migrated row is therefore a scope path, and all four semantic axes
-- are null; only the operational `recorded_at` is populated, from the legacy
-- `time` column, which was always the write clock.
--
-- `entity_id` resolves to the narrow temporal entity key, and the pre-check
-- refuses any scope row whose entity has no temporal entity -- possible only
-- because 0023 dropped this table's foreign key.
--
-- The digest below covers every migrated column, on the same construction 0051
-- uses and for the same reason: both sides are mapped into the rebuilt column
-- shape and unioned under a side marker, so the canonical text is written once
-- and cannot drift between the side it validates and the side it validates
-- against. `recorded_at` is in it because it is the one time axis a migrated
-- scope row carries any information on -- a digest that omitted it would pass
-- over a copy that had restamped every row -- and the synthesised write
-- identity, the ordinal and the `value_kind` are in it because each is asserted
-- by the insert above rather than read from the source, so nothing else would
-- notice if the insert stopped producing them. Nullable members go through
-- `quote_nullable`, which distinguishes a null from the empty string.
-- ============================================================================

SELECT CAST('scope_history row for entity ' || sh.entity_id
            || ' has no temporal entity' AS SMALLINT)
FROM scope_history_legacy AS sh
WHERE NOT EXISTS (SELECT 1 FROM temporal_entities AS te WHERE te.id = sh.entity_id)
LIMIT 1;

INSERT INTO scope_history (
    recorded_at, temporal_entity_key, write_id, write_ordinal, value_kind,
    observed_at, created_at, modified_at, deleted_at,
    generated_instance_uuid, verbatim_instance_id, scope_path)
WITH legacy AS (
    SELECT sh."time" AS legacy_time,
           sh.entity_id,
           sh.scope_path,
           sh.instance_id,
           te.entity_key,
           sh.instance_id ~ ('^urn:ngsi-ld:scope:instance:'
               || '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$')
               AS instance_id_decodes
    FROM scope_history_legacy AS sh
    JOIN temporal_entities AS te ON te.id = sh.entity_id
)
SELECT
    l.legacy_time,
    l.entity_key,
    md5(l.entity_id || '|' || l.instance_id || '|' || l.legacy_time::text)::uuid,
    0,
    1::smallint,
    NULL::timestamptz,
    NULL::timestamptz,
    NULL::timestamptz,
    NULL::timestamptz,
    CASE WHEN l.instance_id_decodes THEN substring(l.instance_id FROM 28)::uuid END,
    CASE WHEN l.instance_id_decodes THEN NULL ELSE l.instance_id END,
    l.scope_path
FROM legacy AS l;

SELECT CAST('scope_history backfill produced ' || rebuilt.n::text
            || ' rows from ' || legacy.n::text AS SMALLINT)
FROM (SELECT count(*) AS n FROM scope_history_legacy) AS legacy,
     (SELECT count(*) AS n FROM scope_history) AS rebuilt
WHERE legacy.n <> rebuilt.n;

WITH canonical AS (
    SELECT 'legacy' AS side,
           te.entity_key,
           sh."time" AS recorded_at,
           md5(sh.entity_id || '|' || sh.instance_id || '|' || sh."time"::text)::uuid AS write_id,
           0 AS write_ordinal,
           1::smallint AS value_kind,
           CASE WHEN sh.instance_id ~ ('^urn:ngsi-ld:scope:instance:'
                    || '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$')
                THEN NULL ELSE sh.instance_id END AS verbatim_instance_id,
           CASE WHEN sh.instance_id ~ ('^urn:ngsi-ld:scope:instance:'
                    || '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$')
                THEN substring(sh.instance_id FROM 28)::uuid END AS generated_instance_uuid,
           sh.scope_path
    FROM scope_history_legacy AS sh
    JOIN temporal_entities AS te ON te.id = sh.entity_id
    UNION ALL
    SELECT 'rebuilt',
           s.temporal_entity_key,
           s.recorded_at,
           s.write_id,
           s.write_ordinal,
           s.value_kind,
           s.verbatim_instance_id,
           s.generated_instance_uuid,
           s.scope_path
    FROM scope_history AS s
),
rendered AS (
    SELECT c.side,
           c.entity_key,
           COALESCE(c.verbatim_instance_id,
                    'urn:ngsi-ld:scope:instance:' || c.generated_instance_uuid::text)
               AS instance_id,
           c.recorded_at,
           c.write_id,
           c.write_ordinal,
           quote_nullable(COALESCE(c.verbatim_instance_id,
                    'urn:ngsi-ld:scope:instance:' || c.generated_instance_uuid::text))
               || E'\x01' || quote_nullable(c.scope_path::text)
               || E'\x01' || quote_nullable(c.value_kind::text)
               || E'\x01' || quote_nullable(c.recorded_at::text)
               || E'\x01' || quote_nullable(c.write_id::text)
               || E'\x01' || quote_nullable(c.write_ordinal::text)
               AS row_text
    FROM canonical AS c
),
digested AS (
    -- `COLLATE "C"` for the same reason 0051 states it: the legacy instance
    -- column and the rebuilt one carry different collations, and naming the
    -- byte-wise one pins the aggregation order to a single, deterministic
    -- choice that is identical on every cluster.
    SELECT r.side,
           r.entity_key,
           count(*) AS n,
           md5(string_agg(r.row_text, E'\x02'
                          ORDER BY r.instance_id COLLATE "C", r.recorded_at,
                                   r.write_id, r.write_ordinal)) AS digest
    FROM rendered AS r
    GROUP BY r.side, r.entity_key
)
SELECT CAST('scope_history backfill diverges on temporal entity '
            || COALESCE(legacy_side.entity_key, rebuilt_side.entity_key)::text AS SMALLINT)
FROM (SELECT entity_key, n, digest FROM digested WHERE side = 'legacy') AS legacy_side
FULL JOIN (SELECT entity_key, n, digest FROM digested WHERE side = 'rebuilt') AS rebuilt_side
       ON rebuilt_side.entity_key = legacy_side.entity_key
WHERE legacy_side.n IS DISTINCT FROM rebuilt_side.n
   OR legacy_side.digest IS DISTINCT FROM rebuilt_side.digest
LIMIT 1;
