-- ============================================================================
-- 0050: Backfill the temporal catalogue
-- ============================================================================
-- One `temporal_entities` row per Entity that has temporal state -- catalogue
-- rows, scope history, or both -- and one `temporal_attribute_streams` row per
-- legacy catalogue entry.
--
-- Existing stream identifiers are preserved verbatim: a stream takes the
-- legacy catalogue row's UUID unchanged, so every legacy history row's
-- `tea_id` is already a valid `stream_id` and the hypertable's join key never
-- has to be rewritten. Only streams minted after the cutover are UUIDv7.
--
-- The two system timestamps come from `entities`, which are recorded values,
-- not inventions. An entity with no type cannot seed a temporal entity, since
-- the types column is checked non-empty; the pre-check reports which entity
-- rather than letting the constraint report only that one exists.
--
-- An entity with catalogue rows but no `entities` row cannot exist: the legacy
-- catalogue's `entity_id` cascades from `entities`. Legacy scope history lost
-- that foreign key in 0023 and is covered by the orphan pre-check in 0052.
-- ============================================================================

SELECT CAST('entity ' || e.id || ' carries no type and cannot seed temporal_entities' AS SMALLINT)
FROM entities AS e
WHERE cardinality(e.types) = 0
  AND (EXISTS (SELECT 1 FROM temporal_entity_attributes AS tea WHERE tea.entity_id = e.id)
    OR EXISTS (SELECT 1 FROM scope_history_legacy AS sh WHERE sh.entity_id = e.id))
LIMIT 1;

INSERT INTO temporal_entities (created_at, modified_at, id, types)
SELECT e.created_at, e.modified_at, e.id, e.types
FROM entities AS e
WHERE EXISTS (SELECT 1 FROM temporal_entity_attributes AS tea WHERE tea.entity_id = e.id)
   OR EXISTS (SELECT 1 FROM scope_history_legacy AS sh WHERE sh.entity_id = e.id);

INSERT INTO temporal_attribute_streams (id, temporal_entity_key, attribute_name_id, dataset_id)
SELECT tea.id, te.entity_key, tea.attribute_name_id, tea.dataset_id
FROM temporal_entity_attributes AS tea
JOIN temporal_entities AS te ON te.id = tea.entity_id;

SELECT CAST('temporal_attribute_streams backfill produced ' || rebuilt.n::text
            || ' rows from ' || legacy.n::text || ' catalogue rows' AS SMALLINT)
FROM (SELECT count(*) AS n FROM temporal_entity_attributes) AS legacy,
     (SELECT count(*) AS n FROM temporal_attribute_streams) AS rebuilt
WHERE legacy.n <> rebuilt.n;
