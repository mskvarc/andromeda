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

WITH legacy AS (
    SELECT te.entity_key,
           count(*) AS n,
           md5(string_agg(sh.instance_id || E'\x01' || sh.scope_path::text,
                          E'\x02' ORDER BY sh.instance_id, sh."time")) AS digest
    FROM scope_history_legacy AS sh
    JOIN temporal_entities AS te ON te.id = sh.entity_id
    GROUP BY te.entity_key
),
rebuilt AS (
    SELECT s.temporal_entity_key AS entity_key,
           count(*) AS n,
           md5(string_agg(
               COALESCE(s.verbatim_instance_id,
                        'urn:ngsi-ld:scope:instance:' || s.generated_instance_uuid::text)
               || E'\x01' || s.scope_path::text,
               E'\x02' ORDER BY COALESCE(s.verbatim_instance_id,
                        'urn:ngsi-ld:scope:instance:' || s.generated_instance_uuid::text),
                        s.recorded_at)) AS digest
    FROM scope_history AS s
    GROUP BY s.temporal_entity_key
)
SELECT CAST('scope_history backfill diverges on temporal entity '
            || COALESCE(legacy.entity_key, rebuilt.entity_key)::text AS SMALLINT)
FROM legacy FULL JOIN rebuilt ON rebuilt.entity_key = legacy.entity_key
WHERE legacy.n IS DISTINCT FROM rebuilt.n
   OR legacy.digest IS DISTINCT FROM rebuilt.digest
LIMIT 1;
