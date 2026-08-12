-- ============================================================================
-- 0039: Byte-comparable, length-bounded attribute-name dictionary
-- ============================================================================
-- `attribute_names.name` holds expanded attribute IRIs. They are identifiers,
-- not prose: nothing about them has linguistic ordering semantics, every
-- comparison the broker makes on them is equality or an anchored prefix, and
-- they are looked up through a unique btree on every write of a new attribute.
-- COLLATE "C" gives that btree byte-wise comparison, makes anchored patterns
-- compile to range scans without a pattern-operator index, enables abbreviated
-- keys in sorts, and removes the standing hazard of a libc or ICU collation
-- change invalidating the index across an operating-system upgrade.
--
-- The length bound is the other half of the dictionary guard. The key was
-- widened to INTEGER in 0035, which removes the 32,767 ceiling but leaves the
-- table append-only and open to inflation by any client that mints distinct
-- attribute IRIs. A per-name length bound caps what a single hostile name can
-- cost the heap and the unique index; capping the *rate* of new names is an
-- API-layer concern and is not attempted here. 2048 is migration-owned and must
-- equal the bound the request layer enforces before a name reaches this table,
-- so an over-long name is refused with an NGSI-LD error rather than a constraint
-- violation surfacing as a persistence failure.
--
-- Changing a column's collation rewrites the table and rebuilds the unique
-- index. `attribute_names` is catalogue-scale -- one row per distinct attribute
-- IRI in the tenant, not one per observation -- so this runs in one transaction
-- with the constraint, and its lock is held for a scan of a few thousand rows.
-- ============================================================================

ALTER TABLE attribute_names ALTER COLUMN name TYPE TEXT COLLATE "C";

ALTER TABLE attribute_names DROP CONSTRAINT IF EXISTS chk_attribute_names_name_length;

ALTER TABLE attribute_names
    ADD CONSTRAINT chk_attribute_names_name_length
    CHECK (char_length(name) BETWEEN 1 AND 2048);
