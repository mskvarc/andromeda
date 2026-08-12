-- ============================================================================
-- 0065: Canonicalize stored NGSI-LD `Time` values in the current-state projection
-- ============================================================================
-- Clause 4.6.3 defines `Time` as an IETF RFC 3339 `full-time`, which pins a time
-- of day exactly but admits many spellings of the same one: the offset may be `Z`
-- or any numeric zone, and the fractional part may be absent or carry any number
-- of digits. Until this change the write path stored whichever spelling the
-- client wrote.
--
-- Clause 4.9 orders `Time` values chronologically with `<`, `>`, `<=` and `>=`.
-- Those comparisons are answered from `current_attribute_values.value_string`,
-- which 0057 declares `COLLATE "C"` and which is therefore ordered byte-wise. A
-- byte-wise order over arbitrary spellings is not a chronological order:
-- `12:00:00Z` sorts after `11:00:00+01:00` even though the two name the same time
-- of day, and `12:00:00.5Z` sorts before `12:00:00Z` even though it is later.
--
-- The broker now stores exactly one rendering — `HH:MM:SS.ffffffZ`: fixed width,
-- always six fractional digits, always `Z` — which is the form byte-wise order
-- and chronological order agree on. This migration brings rows written before
-- that into the same form.
--
-- Identifying the rows. A `Time` value is a string, and the projection stores
-- strings in the same column whatever they mean, so the projection alone cannot
-- say which rows are `Time` values. The stored expanded payload can: JSON-LD
-- carries the declared type on the value node as
-- `hasValue[0]."@type" = "https://uri.etsi.org/ngsi-ld/Time"`. That is what is
-- read here. Nothing is matched by the *shape* of the stored text — a Property
-- whose value happens to be the string `12:00:00Z` is an ordinary string, and
-- rewriting it would silently change a value the client wrote.
--
-- Matching the projection row. Clause 4.5.5 identifies an attribute instance by
-- (Entity, attribute name, `datasetId`), with the default instance being the one
-- carrying no `datasetId`, so the payload instance and the projection row are
-- matched on all three with `IS NOT DISTINCT FROM` on the nullable member.
--
-- `entities.payload` is the expanded JSON-LD document as a one-element array,
-- which is the shape every `q` jsonpath in the query layer already addresses as
-- `$[0]`; `payload -> 0` is the same reading. `jsonb_each` over a non-object
-- yields no rows, so a payload of any other shape contributes nothing rather than
-- failing the migration.
--
-- Rows whose text is not a real RFC 3339 `full-time` at all are left untouched:
-- there is no canonical form to move them to, and refusing them here would block
-- the migration on data an earlier broker accepted. They stay comparable as
-- text, as they were, and the next write of that attribute refuses them.
--
-- "Not a real `full-time`" is checked two ways, both of which must pass before a
-- row is rewritten. A shape regex restricts the text to clause 4.6.3's
-- `full-time` production, so `12:00:00Z` matches and `soon` does not. Shape
-- alone is not enough: `25:00:00Z` and `12:60:00Z` have the right shape but name
-- an hour and a minute a day does not have, and casting them aborts the
-- migration rather than declining to touch that one row. `pg_input_is_valid`
-- (PostgreSQL 16+) asks the `timestamptz` input function itself whether the
-- anchored text is a real instant, without raising, so it catches exactly the
-- rows the shape regex cannot.
--
-- History is deliberately not rewritten. `attribute_history.value_string` holds
-- the same values for `Time` observations, and the same argument applies to
-- clause 4.5.19's `min` and `max` over them. It is left alone here for two
-- reasons: the history relation is a Timescale hypertable whose rewrite is
-- unbounded in the size of the retained corpus, which is not a cost this
-- correctness fix should impose on a running tenant; and history is an append-only
-- record of what was observed, so rewriting past observations is a different kind
-- of change from correcting a projection that is derived state by construction.
-- The consequence, recorded rather than discovered: a temporal `min`/`max` over a
-- `Time` attribute written before this migration can disagree with a current-state
-- `q` over the same attribute until every instance has been rewritten. Closing it
-- needs its own migration with its own rewrite budget.
-- ============================================================================

WITH time_grain AS (
    SELECT DISTINCT
        e.entity_key,
        an.id AS attribute_name_id,
        (instance -> 'https://uri.etsi.org/ngsi-ld/datasetId' -> 0 ->> '@id') AS dataset_id
    FROM entities AS e
    CROSS JOIN LATERAL jsonb_each(e.payload -> 0) AS member(name, instances)
    JOIN attribute_names AS an ON an.name = member.name
    CROSS JOIN LATERAL jsonb_array_elements(
        CASE WHEN jsonb_typeof(member.instances) = 'array' THEN member.instances ELSE '[]'::jsonb END) AS instance
    WHERE instance -> 'https://uri.etsi.org/ngsi-ld/hasValue' -> 0 ->> '@type'
          = 'https://uri.etsi.org/ngsi-ld/Time'
),
canonical AS (
    SELECT
        g.entity_key,
        g.attribute_name_id,
        g.dataset_id,
        -- The time of day is anchored to a date so the `timestamptz` parser
        -- resolves its offset, and converted to UTC before the date is dropped.
        -- The literal carries its own offset, so neither step consults the
        -- session `TimeZone`.
        to_char(('1970-01-01T' || c.value_string)::timestamptz AT TIME ZONE 'UTC', 'HH24:MI:SS.US') || 'Z' AS value_string
    FROM time_grain AS g
    JOIN current_attribute_values AS c
      ON c.entity_key = g.entity_key
     AND c.attribute_name_id = g.attribute_name_id
     AND c.dataset_id IS NOT DISTINCT FROM g.dataset_id
    WHERE c.value_string ~ '^[0-9]{2}:[0-9]{2}:[0-9]{2}([.][0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})$'
      AND pg_input_is_valid('1970-01-01T' || c.value_string, 'timestamptz')
)
UPDATE current_attribute_values AS c
SET value_string = k.value_string
FROM canonical AS k
WHERE c.entity_key = k.entity_key
  AND c.attribute_name_id = k.attribute_name_id
  AND c.dataset_id IS NOT DISTINCT FROM k.dataset_id
  AND c.value_string IS DISTINCT FROM k.value_string;

-- Assert the result rather than assume it: every projection row the payload
-- declares to be a `Time` value, and whose text is a real `full-time` at all,
-- must now be in the canonical form. A row that is not means the rewrite missed
-- a grain. The same two-part check the rewrite used gates the assertion too —
-- otherwise a row the rewrite deliberately left alone, because its text has the
-- right shape but names no real time of day, would be flagged here as a missed
-- grain instead of the untouched row it correctly is.
SELECT CAST('current_attribute_values row for entity_key ' || c.entity_key::text
            || ' attribute ' || c.attribute_name_id::text
            || ' holds a non-canonical Time value' AS SMALLINT)
FROM current_attribute_values AS c
JOIN entities AS e ON e.entity_key = c.entity_key
CROSS JOIN LATERAL jsonb_each(e.payload -> 0) AS member(name, instances)
JOIN attribute_names AS an ON an.id = c.attribute_name_id AND an.name = member.name
CROSS JOIN LATERAL jsonb_array_elements(
    CASE WHEN jsonb_typeof(member.instances) = 'array' THEN member.instances ELSE '[]'::jsonb END) AS instance
WHERE instance -> 'https://uri.etsi.org/ngsi-ld/hasValue' -> 0 ->> '@type'
      = 'https://uri.etsi.org/ngsi-ld/Time'
  AND c.dataset_id IS NOT DISTINCT FROM (instance -> 'https://uri.etsi.org/ngsi-ld/datasetId' -> 0 ->> '@id')
  AND c.value_string ~ '^[0-9]{2}:[0-9]{2}:[0-9]{2}([.][0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})$'
  AND pg_input_is_valid('1970-01-01T' || c.value_string, 'timestamptz')
  AND c.value_string !~ '^[0-9]{2}:[0-9]{2}:[0-9]{2}[.][0-9]{6}Z$'
LIMIT 1;
