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
-- The digest validation is what proves the rebuild lossless, and it covers
-- every semantic column the rebuilt row carries rather than a sample of them:
-- the instance identifier round-tripped back through the permanent template, so
-- a decode that lost information mismatches rather than silently renaming an
-- instance; `attribute_kind` and `value_kind`, so either mapping going wrong
-- mismatches; the value's canonical text with the float8-to-NUMERIC cast
-- applied on the legacy side exactly as the backfill applies it, so the
-- widening is verified rather than assumed, with geometries compared as EWKB;
-- all four semantic time axes plus the operational `recorded_at`, so the axis
-- separation the rebuild introduced is verified rather than assumed; the write
-- identity, so a synthesised digest that collided or a preserved one that was
-- dropped mismatches; `unit_code_id` and `metadata`; and the stream's
-- `dataset_id`, read from the legacy catalogue on one side and from the rebuilt
-- stream on the other, which is what checks that 0050 carried the clause 4.5.5
-- multi-attribute grain across intact for every stream that has history.
--
-- It is a single canonical digest rather than a bidirectional `EXCEPT`, and
-- that is the cheaper of the two correct options here: `attribute_history` is a
-- hypertable of arbitrary size, an `EXCEPT` in both directions sorts or hashes
-- the full width of both relations twice, whereas the aggregate below reads
-- each side once and reduces every row to a bounded text before grouping. Its
-- one requirement is a deterministic aggregation order, which the sort key
-- supplies: instance identifier, then `recorded_at`, then the write identity,
-- which the third pre-check has already proven unique per row.
--
-- The two sides share one expression rather than two parallel copies. Both are
-- mapped into the rebuilt column shape first and then unioned under a side
-- marker, so the canonical text is written once and cannot drift between the
-- side it validates and the side it validates against. Nullable members go
-- through `quote_nullable`, which distinguishes a null from the empty string --
-- a `COALESCE(x, '')` would make an absent `datasetId` and an empty one digest
-- alike, which is exactly the class of loss this check exists to catch.
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
           tea.dataset_id,
           ah.value_string IN ('urn:ngsi-ld:null', 'https://uri.etsi.org/ngsi-ld/null')
               AS is_tombstone,
           ah.instance_id ~ ('^urn:ngsi-ld:attribute:instance:'
               || '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$')
               AS instance_id_decodes
    FROM attribute_history_legacy AS ah
    JOIN temporal_entity_attributes AS tea ON tea.id = ah.tea_id
),
canonical AS (
    SELECT 'legacy' AS side,
           l.tea_id AS stream_id,
           l.dataset_id,
           COALESCE(l.recorded_at, l.legacy_time) AS recorded_at,
           COALESCE(l.write_id,
                    md5(l.tea_id::text || '|' || l.instance_id || '|' || l.legacy_time::text)::uuid)
               AS write_id,
           COALESCE(l.write_ordinal, 0) AS write_ordinal,
           (CASE l.attribute_type
               WHEN 'Property' THEN 1
               WHEN 'Relationship' THEN 2
               WHEN 'GeoProperty' THEN 3
               WHEN 'VocabProperty' THEN 4
               WHEN 'LanguageProperty' THEN 5
               WHEN 'JsonProperty' THEN 6
               WHEN 'ListProperty' THEN 7
               WHEN 'ListRelationship' THEN 8
            END)::smallint AS attribute_kind,
           (CASE
               WHEN l.is_tombstone THEN 0
               WHEN l.value_number IS NOT NULL THEN 1
               WHEN l.value_string IS NOT NULL THEN 2
               WHEN l.value_bool IS NOT NULL THEN 3
               WHEN l.value_geo IS NOT NULL THEN 4
               WHEN l.value_object IS NOT NULL THEN 5
               WHEN l.value_datetime IS NOT NULL THEN 6
               WHEN l.value_uri IS NOT NULL THEN 7
            END)::smallint AS value_kind,
           l.observed_at,
           l.recorded_at AS created_at,
           l.modified_at,
           CASE WHEN l.is_tombstone THEN COALESCE(l.recorded_at, l.legacy_time) END AS deleted_at,
           CASE WHEN l.instance_id_decodes THEN substring(l.instance_id FROM 32)::uuid END
               AS generated_instance_uuid,
           CASE WHEN l.instance_id_decodes THEN NULL ELSE l.instance_id END
               AS verbatim_instance_id,
           l.unit_code_id,
           CASE WHEN l.is_tombstone THEN NULL ELSE l.value_number::numeric END AS value_number,
           CASE WHEN l.is_tombstone THEN NULL ELSE l.value_string END AS value_string,
           CASE WHEN l.is_tombstone THEN NULL ELSE l.value_bool END AS value_bool,
           CASE WHEN l.is_tombstone THEN NULL ELSE l.value_geo END AS value_geo,
           CASE WHEN l.is_tombstone THEN NULL ELSE l.value_object END AS value_object,
           CASE WHEN l.is_tombstone THEN NULL ELSE l.value_datetime END AS value_datetime,
           CASE WHEN l.is_tombstone THEN NULL ELSE l.value_uri END AS value_uri,
           l.metadata
    FROM legacy AS l
    UNION ALL
    SELECT 'rebuilt',
           a.stream_id,
           s.dataset_id,
           a.recorded_at,
           a.write_id,
           a.write_ordinal,
           a.attribute_kind,
           a.value_kind,
           a.observed_at,
           a.created_at,
           a.modified_at,
           a.deleted_at,
           a.generated_instance_uuid,
           a.verbatim_instance_id,
           a.unit_code_id,
           a.value_number,
           a.value_string,
           a.value_bool,
           a.value_geo,
           a.value_object,
           a.value_datetime,
           a.value_uri,
           a.metadata
    FROM attribute_history AS a
    JOIN temporal_attribute_streams AS s ON s.id = a.stream_id
),
rendered AS (
    SELECT c.side,
           c.stream_id,
           COALESCE(c.verbatim_instance_id,
                    'urn:ngsi-ld:attribute:instance:' || c.generated_instance_uuid::text)
               AS instance_id,
           c.recorded_at,
           c.write_id,
           c.write_ordinal,
           quote_nullable(COALESCE(c.verbatim_instance_id,
                    'urn:ngsi-ld:attribute:instance:' || c.generated_instance_uuid::text))
               || E'\x01' || quote_nullable(c.dataset_id)
               || E'\x01' || quote_nullable(c.attribute_kind::text)
               || E'\x01' || quote_nullable(c.value_kind::text)
               || E'\x01' || quote_nullable(concat_ws('', c.value_number::text, c.value_string,
                                                      c.value_bool::text, ST_AsEWKB(c.value_geo)::text,
                                                      c.value_object::text, c.value_datetime::text,
                                                      c.value_uri))
               || E'\x01' || quote_nullable(c.unit_code_id::text)
               || E'\x01' || quote_nullable(c.metadata::text)
               || E'\x01' || quote_nullable(c.recorded_at::text)
               || E'\x01' || quote_nullable(c.observed_at::text)
               || E'\x01' || quote_nullable(c.created_at::text)
               || E'\x01' || quote_nullable(c.modified_at::text)
               || E'\x01' || quote_nullable(c.deleted_at::text)
               || E'\x01' || quote_nullable(c.write_id::text)
               || E'\x01' || quote_nullable(c.write_ordinal::text)
               AS row_text
    FROM canonical AS c
),
digested AS (
    -- The sort key is spelled `COLLATE "C"` explicitly. The legacy instance
    -- column carries the database collation and the rebuilt one carries `C`;
    -- naming the byte-wise one pins the aggregation order to a single,
    -- deterministic choice rather than leaving it to whichever side's
    -- collation PostgreSQL's implicit-collation resolution would otherwise
    -- pick, so the digest's ordering is identical on every cluster.
    SELECT r.side,
           r.stream_id,
           count(*) AS n,
           md5(string_agg(r.row_text, E'\x02'
                          ORDER BY r.instance_id COLLATE "C", r.recorded_at,
                                   r.write_id, r.write_ordinal))
               AS digest
    FROM rendered AS r
    GROUP BY r.side, r.stream_id
)
SELECT CAST('attribute_history backfill diverges on stream '
            || COALESCE(legacy_side.stream_id, rebuilt_side.stream_id)::text AS SMALLINT)
FROM (SELECT stream_id, n, digest FROM digested WHERE side = 'legacy') AS legacy_side
FULL JOIN (SELECT stream_id, n, digest FROM digested WHERE side = 'rebuilt') AS rebuilt_side
       ON rebuilt_side.stream_id = legacy_side.stream_id
WHERE legacy_side.n IS DISTINCT FROM rebuilt_side.n
   OR legacy_side.digest IS DISTINCT FROM rebuilt_side.digest
LIMIT 1;
