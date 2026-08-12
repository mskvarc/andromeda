-- ============================================================================
-- 0054: Drop the legacy temporal relations and every trigger behind them
-- ============================================================================
-- The rebuilt relations are loaded, validated and frozen, so the legacy ones
-- and the procedural code that maintained them go.
--
-- Both cascade triggers exist only because 0023 removed the hypertables'
-- foreign keys and replaced them with statement-level PL/pgSQL. The rebuilt
-- schema has no cascade from current state onto history at all: Core Entity
-- deletion (clause 5.6.6) leaves temporal state standing and records
-- tombstones, and Temporal Evolution deletion (clause 5.6.16) deletes history,
-- then streams, then the temporal entity in one Rust transaction with an orphan
-- audit as the safety net.
--
-- `remove_expired_attribute_instances` has had no caller since expiry moved
-- into Rust. It is dropped, not deprecated.
--
-- `attribute_current`'s foreign key to the legacy catalogue is dropped so the
-- catalogue can go. That leaves the projection keyed on a UUID with nothing
-- behind it until it is re-keyed onto the stream grain.
--
-- No IF EXISTS anywhere: every object named here is known to exist at this
-- point, and a missing one means the database is not in the state the preceding
-- migrations left it in, which must fail rather than be tolerated.
--
-- The trigger assertion anti-joins through pg_depend on the trigger's
-- *function* rather than filtering tgisinternal, because TimescaleDB's
-- insert blocker on every hypertable root is a user-visible trigger whose
-- function is extension-owned. The same anti-join is what lets the pg_proc
-- assertion ignore PostGIS, ltree and TimescaleDB while catching anything this
-- schema owns. After this file `geom_to_geog` -- one immutable single
-- expression -- is the only function the schema owns, and it owns no triggers.
-- ============================================================================

ALTER TABLE attribute_current DROP CONSTRAINT attribute_current_tea_id_fkey;

DROP TRIGGER tea_cascade_attribute_history ON temporal_entity_attributes;
DROP TRIGGER entity_cascade_scope_history ON entities;

DROP FUNCTION trg_tea_cascade_attribute_history();
DROP FUNCTION trg_entity_cascade_scope_history();
DROP FUNCTION remove_expired_attribute_instances(JSONB, TIMESTAMPTZ);

DROP TABLE attribute_history_legacy;
DROP TABLE scope_history_legacy;
DROP TABLE temporal_entity_attributes;
DROP TABLE temporal_history_sink_receipts;

SELECT CAST('schema still owns a trigger: ' || t.tgname AS SMALLINT)
FROM pg_trigger AS t
JOIN pg_class AS c ON c.oid = t.tgrelid
JOIN pg_namespace AS n ON n.oid = c.relnamespace
LEFT JOIN pg_depend AS d ON d.objid = t.tgfoid AND d.deptype = 'e'
WHERE n.nspname = 'public' AND NOT t.tgisinternal AND d.objid IS NULL
LIMIT 1;

SELECT CAST('schema still owns procedural code: ' || p.proname AS SMALLINT)
FROM pg_proc AS p
JOIN pg_namespace AS n ON n.oid = p.pronamespace
LEFT JOIN pg_depend AS d ON d.objid = p.oid AND d.deptype = 'e'
WHERE n.nspname = 'public' AND d.objid IS NULL AND p.proname <> 'geom_to_geog'
LIMIT 1;
