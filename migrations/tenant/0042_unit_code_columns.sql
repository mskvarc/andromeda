-- ============================================================================
-- 0042: Give attributes a unit-code column and empty the member out of metadata
-- ============================================================================
-- Until now the `unitCode` member (NGSI-LD 5.2.5) lived inside the `metadata`
-- JSONB document of every attribute instance, which costs a repeated key IRI
-- and a repeated code per row and makes a unit-aware predicate a JSON traversal
-- rather than a column read. Both facts are measured in
-- `docs/proposals/database-schema-redesign-benchmarks.md`; the column is the
-- conclusion.
--
-- `attribute_current` is an ordinary table, so it takes a foreign key to the
-- catalogue -- the catalogue is a few thousand rows, entirely cache-resident,
-- and the projection is written far less often than the history. The
-- `attribute_history` hypertable takes none: 0023 removed the hypertable
-- foreign keys deliberately, and the guarantee is restored in Rust (the only
-- writer resolves the code through the compiled-in catalogue) plus the audit
-- query `TemporalRepository::audit_unregistered_unit_codes`. That the audit is
-- cheaper than the constraint is the proposal's untested default, recorded here
-- as such rather than as a measurement.
--
-- The backfill and the strip are one statement per table so a row can never be
-- observed with the unit in both places. A row whose member is not the single
-- `@value` entry the writer produces, or whose code the catalogue does not
-- register, matches nothing and keeps its member; the assertion below then
-- fails the migration rather than letting the broker meet a shape its decoder
-- refuses. The assertion casts a sentence to SMALLINT and so only errors when
-- the offending set is non-empty -- there is no procedural code in this schema
-- to raise with. The sentence must reference a column: a wholly constant cast
-- is folded during planning and would fail the migration even on an empty
-- table, so the offending `tea_id` is concatenated in and doubles as the
-- diagnostic.
--
-- `attribute_current` carries no metadata document of its own; its unit comes
-- from the newest history row of the same attribute, which is the instance the
-- projection mirrors.
--
-- The backfills invalidate the statistics of the tables they rewrite, hence the
-- closing ANALYZE. Freezing those heaps is left to autovacuum: the migration
-- runner sends a migration as a single multi-statement simple query, which
-- PostgreSQL executes inside an implicit transaction block, and VACUUM cannot
-- run there. Every statement is individually re-runnable: the adds are
-- `IF NOT EXISTS`, the backfills skip rows they already filled, and the
-- constraint is dropped before it is added.
-- ============================================================================

ALTER TABLE attribute_current ADD COLUMN IF NOT EXISTS unit_code_id SMALLINT;

ALTER TABLE attribute_history ADD COLUMN IF NOT EXISTS unit_code_id SMALLINT;

UPDATE attribute_history AS ah
SET unit_code_id = uc.id,
    metadata = NULLIF(ah.metadata - 'https://uri.etsi.org/ngsi-ld/unitCode', '{}'::jsonb)
FROM unit_codes AS uc
WHERE ah.unit_code_id IS NULL
  AND ah.metadata ? 'https://uri.etsi.org/ngsi-ld/unitCode'
  AND uc.code = ah.metadata -> 'https://uri.etsi.org/ngsi-ld/unitCode' -> 0 ->> '@value';

SELECT CAST('attribute_history.metadata still holds a unitCode member the catalogue cannot map, tea_id ' || tea_id::text AS SMALLINT)
FROM attribute_history
WHERE metadata ? 'https://uri.etsi.org/ngsi-ld/unitCode'
LIMIT 1;

UPDATE attribute_current AS ac
SET unit_code_id = newest.unit_code_id
FROM (
    SELECT DISTINCT ON (tea_id) tea_id, unit_code_id
    FROM attribute_history
    ORDER BY tea_id, time DESC, instance_id DESC
) AS newest
WHERE newest.tea_id = ac.tea_id
  AND newest.unit_code_id IS NOT NULL
  AND ac.unit_code_id IS NULL;

ALTER TABLE attribute_current DROP CONSTRAINT IF EXISTS fk_attribute_current_unit_code;

ALTER TABLE attribute_current
    ADD CONSTRAINT fk_attribute_current_unit_code
    FOREIGN KEY (unit_code_id) REFERENCES unit_codes(id) NOT VALID;

ALTER TABLE attribute_current VALIDATE CONSTRAINT fk_attribute_current_unit_code;

ANALYZE unit_codes;

ANALYZE attribute_history;

ANALYZE attribute_current;
