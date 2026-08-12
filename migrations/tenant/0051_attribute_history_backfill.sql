-- ============================================================================
-- 0051: Backfill the rebuilt attribute history
-- ============================================================================
-- Column mapping, legacy to rebuilt:
--
--   time            -> recorded_at, and only where recorded_at is null. The old
--                      column was "observedAt if the client sent one, else the
--                      write clock". It is the closest available basis for the
--                      operational axis and is used for nothing else.
--   time            -> deleted_at, tombstones only. A tombstone must carry a
--                      deletion instant by check constraint; a deletion has no
--                      observedAt, so the old `time` on a tombstone row is the
--                      write clock.
--   recorded_at     -> recorded_at, and -> created_at, nulls preserved. Never
--                      synthesised from `time`.
--   modified_at     -> modified_at, nulls preserved.
--   observed_at     -> observed_at, nulls preserved. Never synthesised: a row
--                      with no stored observedAt matches no observedAt filter.
--   tea_id          -> stream_id, unchanged.
--   write_id        -> write_id when present, else a content digest (below).
--   write_ordinal   -> write_ordinal when present, else 0.
--   instance_id     -> the split instance identity (below).
--   value_number    -> value_number, float8 cast to NUMERIC, which routes
--                      through the shortest round-trip text form of the double.
--   the other typed columns, unit_code_id and metadata -> verbatim. Geometry is
--                      not re-validated and not re-projected.
--   (none)          -> attribute_kind, from the catalogue row's attribute_type.
--   (none)          -> value_kind, from the column that actually holds a value.
--
-- Bounded limitation, deliberate and documented: `attribute_kind` on migrated
-- rows is the *current* kind of the stream, not the kind the row had when it
-- was written. The legacy schema recorded one kind per catalogue entry and
-- overwrote it whenever an attribute's type changed, so no per-row historical
-- kind exists to recover; rows written before a type change render with the
-- post-change kind. This affects only data predating the rebuild -- every row
-- written after it carries the kind that was true at write time, which is why
-- the column exists. `value_kind` carries no such limitation: it is derived per
-- row from the populated column, so it is exact even where the legacy
-- catalogue's value_type disagreed with the column.
--
-- Tombstone detection is textual, because a stored null representation is the
-- only encoding the legacy writer ever produced for a deletion. A client that
-- genuinely stored one of those two strings as a value loses it to a tombstone;
-- the alternative silently resurrects deleted attributes, which is worse.
--
-- Instance identity decodes only from the canonical template: the literal
-- prefix followed by a lowercase hyphenated UUID and nothing else. Everything
-- else -- client-supplied identifiers (clause 4.5.7 requires them to survive),
-- uppercase or braced UUID spellings, and the broker's older two-value
-- `{write_id}:{ordinal}` spelling that one UUID column cannot round-trip --
-- becomes verbatim, which is lossless. The write identity that spelling
-- encoded is on the row anyway.
--
-- A synthesised write identity is a content digest: deterministic, so a re-run
-- produces the same value, and unique given the legacy UNIQUE (tea_id, time,
-- instance_id). It is explicitly not a UUIDv7 and carries no ordering --
-- nothing may infer order from a write_id, `recorded_at` is the ordering axis.
-- The third pre-check refuses any legacy row set that would collide on the
-- rebuilt table's deduplicating key, so a failure names the offending write
-- rather than surfacing as a constraint violation mid-insert.
--
-- The digest validation is what proves the rebuild lossless. It round-trips
-- every instance identifier back through the permanent template, so a decode
-- that lost information mismatches rather than silently renaming an instance;
-- it carries the derived value_kind, so a kind mapping error mismatches; and it
-- carries the value's canonical text with the float8-to-NUMERIC cast applied on
-- the legacy side exactly as the backfill applies it, so the widening is
-- verified rather than assumed. Geometries are compared as EWKB.
-- ============================================================================

SELECT CAST('attribute_history row for stream ' || ah.tea_id::text
            || ' has no catalogue entry' AS SMALLINT)
FROM attribute_history_legacy AS ah
WHERE NOT EXISTS (SELECT 1 FROM temporal_attribute_streams AS s WHERE s.id = ah.tea_id)
LIMIT 1;

SELECT CAST('attribute_history row for stream ' || ah.tea_id::text
            || ' populates no typed value column' AS SMALLINT)
FROM attribute_history_legacy AS ah
WHERE num_nonnulls(ah.value_number, ah.value_string, ah.value_bool, ah.value_geo,
                   ah.value_object, ah.value_datetime, ah.value_uri) <> 1
LIMIT 1;

SELECT CAST('attribute_history repeats a write identity at write_id '
            || ah.write_id::text AS SMALLINT)
FROM attribute_history_legacy AS ah
WHERE ah.write_id IS NOT NULL AND ah.write_ordinal IS NOT NULL
GROUP BY COALESCE(ah.recorded_at, ah."time"), ah.write_id, ah.write_ordinal
HAVING count(*) > 1
LIMIT 1;

INSERT INTO attribute_history (
    recorded_at, write_id, stream_id, write_ordinal, attribute_kind, value_kind,
    observed_at, created_at, modified_at, deleted_at, value_datetime,
    generated_instance_uuid, unit_code_id, value_bool,
    verbatim_instance_id, value_number, value_string, value_geo,
    value_object, value_uri, metadata)
WITH legacy AS (
    SELECT ah."time" AS legacy_time,
           ah.tea_id,
           ah.recorded_at,
           ah.modified_at,
           ah.observed_at,
           ah.instance_id,
           ah.write_id,
           ah.write_ordinal,
           ah.unit_code_id,
           ah.metadata,
           ah.value_number,
           ah.value_string,
           ah.value_bool,
           ah.value_geo,
           ah.value_object,
           ah.value_datetime,
           ah.value_uri,
           tea.attribute_type,
           ah.value_string IN ('urn:ngsi-ld:null', 'https://uri.etsi.org/ngsi-ld/null')
               AS is_tombstone,
           ah.instance_id ~ ('^urn:ngsi-ld:attribute:instance:'
               || '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$')
               AS instance_id_decodes
    FROM attribute_history_legacy AS ah
    JOIN temporal_entity_attributes AS tea ON tea.id = ah.tea_id
)
SELECT
    COALESCE(l.recorded_at, l.legacy_time),
    COALESCE(l.write_id,
             md5(l.tea_id::text || '|' || l.instance_id || '|' || l.legacy_time::text)::uuid),
    l.tea_id,
    COALESCE(l.write_ordinal, 0),
    (CASE l.attribute_type
        WHEN 'Property' THEN 1
        WHEN 'Relationship' THEN 2
        WHEN 'GeoProperty' THEN 3
        WHEN 'VocabProperty' THEN 4
        WHEN 'LanguageProperty' THEN 5
        WHEN 'JsonProperty' THEN 6
        WHEN 'ListProperty' THEN 7
        WHEN 'ListRelationship' THEN 8
     END)::smallint,
    (CASE
        WHEN l.is_tombstone THEN 0
        WHEN l.value_number IS NOT NULL THEN 1
        WHEN l.value_string IS NOT NULL THEN 2
        WHEN l.value_bool IS NOT NULL THEN 3
        WHEN l.value_geo IS NOT NULL THEN 4
        WHEN l.value_object IS NOT NULL THEN 5
        WHEN l.value_datetime IS NOT NULL THEN 6
        WHEN l.value_uri IS NOT NULL THEN 7
     END)::smallint,
    l.observed_at,
    l.recorded_at,
    l.modified_at,
    CASE WHEN l.is_tombstone THEN COALESCE(l.recorded_at, l.legacy_time) END,
    CASE WHEN l.is_tombstone THEN NULL ELSE l.value_datetime END,
    CASE WHEN l.instance_id_decodes THEN substring(l.instance_id FROM 32)::uuid END,
    l.unit_code_id,
    CASE WHEN l.is_tombstone THEN NULL ELSE l.value_bool END,
    CASE WHEN l.instance_id_decodes THEN NULL ELSE l.instance_id END,
    CASE WHEN l.is_tombstone THEN NULL ELSE l.value_number::numeric END,
    CASE WHEN l.is_tombstone THEN NULL ELSE l.value_string END,
    CASE WHEN l.is_tombstone THEN NULL ELSE l.value_geo END,
    CASE WHEN l.is_tombstone THEN NULL ELSE l.value_object END,
    CASE WHEN l.is_tombstone THEN NULL ELSE l.value_uri END,
    l.metadata
FROM legacy AS l;

SELECT CAST('attribute_history backfill produced ' || rebuilt.n::text
            || ' rows from ' || legacy.n::text AS SMALLINT)
FROM (SELECT count(*) AS n FROM attribute_history_legacy) AS legacy,
     (SELECT count(*) AS n FROM attribute_history) AS rebuilt
WHERE legacy.n <> rebuilt.n;

WITH legacy AS (
    SELECT ah.tea_id AS stream_id,
           count(*) AS n,
           md5(string_agg(
               ah.instance_id
               || E'\x01'
               || (CASE
                     WHEN ah.value_string IN ('urn:ngsi-ld:null',
                                              'https://uri.etsi.org/ngsi-ld/null') THEN 0
                     WHEN ah.value_number IS NOT NULL THEN 1
                     WHEN ah.value_string IS NOT NULL THEN 2
                     WHEN ah.value_bool IS NOT NULL THEN 3
                     WHEN ah.value_geo IS NOT NULL THEN 4
                     WHEN ah.value_object IS NOT NULL THEN 5
                     WHEN ah.value_datetime IS NOT NULL THEN 6
                     WHEN ah.value_uri IS NOT NULL THEN 7
                   END)::text
               || E'\x01'
               || COALESCE(CASE
                     WHEN ah.value_string IN ('urn:ngsi-ld:null',
                                              'https://uri.etsi.org/ngsi-ld/null') THEN ''
                     ELSE concat_ws('', ah.value_number::numeric::text, ah.value_string,
                                    ah.value_bool::text, ST_AsEWKB(ah.value_geo)::text,
                                    ah.value_object::text, ah.value_datetime::text,
                                    ah.value_uri)
                   END, ''),
               E'\x02' ORDER BY ah.instance_id, COALESCE(ah.recorded_at, ah."time"))) AS digest
    FROM attribute_history_legacy AS ah
    GROUP BY ah.tea_id
),
rebuilt AS (
    SELECT a.stream_id,
           count(*) AS n,
           md5(string_agg(
               COALESCE(a.verbatim_instance_id,
                        'urn:ngsi-ld:attribute:instance:' || a.generated_instance_uuid::text)
               || E'\x01'
               || a.value_kind::text
               || E'\x01'
               || COALESCE(concat_ws('', a.value_number::text, a.value_string,
                                     a.value_bool::text, ST_AsEWKB(a.value_geo)::text,
                                     a.value_object::text, a.value_datetime::text,
                                     a.value_uri), ''),
               E'\x02' ORDER BY COALESCE(a.verbatim_instance_id,
                        'urn:ngsi-ld:attribute:instance:' || a.generated_instance_uuid::text),
                        a.recorded_at)) AS digest
    FROM attribute_history AS a
    GROUP BY a.stream_id
)
SELECT CAST('attribute_history backfill diverges on stream '
            || COALESCE(legacy.stream_id, rebuilt.stream_id)::text AS SMALLINT)
FROM legacy FULL JOIN rebuilt ON rebuilt.stream_id = legacy.stream_id
WHERE legacy.n IS DISTINCT FROM rebuilt.n
   OR legacy.digest IS DISTINCT FROM rebuilt.digest
LIMIT 1;
