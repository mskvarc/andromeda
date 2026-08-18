-- ============================================================================
-- 0007: Name dictionaries -- attribute names and entity type names
-- ============================================================================
-- Two normalization layers between expanded IRIs and every relation that
-- carries one. `attribute_names` sits between attribute IRIs and the relations
-- that carry an attribute identity (temporal streams, current-value
-- projection, registration routing); `entity_type_names` sits between entity
-- type IRIs and the relations that carry an entity's types (`entities`,
-- `temporal_entities`, both as an `INTEGER[]` of keys). Instead of repeating a
-- 60+ char IRI on every row of those relations, each stores a compact INTEGER
-- reference into the dictionary. For the type arrays the saving compounds: the
-- GIN index that serves a clause 4.17 type selection indexes four-byte keys
-- instead of IRIs, and stays a fraction of the size.
--
-- `id` is INTEGER rather than SMALLINT in both: attribute and type IRIs are
-- open-world domain values and NGSI-LD sets no corresponding limit, so a
-- 32,767-key ceiling would be a capacity accident rather than a modelled
-- constraint.
--
-- `name` is `TEXT COLLATE "C"` in both. The IRIs are identifiers, not prose --
-- nothing about them has linguistic ordering semantics, every comparison the
-- broker makes on them is equality or an anchored prefix, and they are looked
-- up through a unique btree on every write of a new name. `COLLATE "C"` gives
-- that btree byte-wise comparison, makes anchored patterns compile to range
-- scans without a pattern-operator index, and removes the standing hazard of a
-- libc or ICU collation change invalidating the index across an
-- operating-system upgrade.
--
-- The length bound caps what a single hostile name can cost the heap and the
-- unique index; capping the *rate* of new names is an API-layer concern and
-- is not attempted here. 2048 is migration-owned and must equal the bound the
-- request layer enforces before a name reaches either table, so an over-long
-- name is refused with an NGSI-LD error rather than a constraint violation
-- surfacing as a persistence failure.
--
-- Both dictionaries stay append-only and therefore open to inflation: any
-- client can mint unbounded distinct IRIs. Rate limiting new-name creation
-- and observing dictionary growth are API-layer concerns and are deliberately
-- absent from this migration; the broker bounds each dictionary's size per
-- tenant before it mints.
--
-- The type arrays carry no foreign key to `entity_type_names`: an array
-- element cannot be a foreign key in PostgreSQL, and the broker mints every
-- key inside the transaction that writes the row referencing it.
-- ============================================================================

CREATE TABLE attribute_names (
    id INTEGER PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
    name TEXT COLLATE "C" NOT NULL,
    CONSTRAINT uq_attribute_names_name UNIQUE (name),
    CONSTRAINT chk_attribute_names_name_length CHECK (char_length(name) BETWEEN 1 AND 2048)
);

CREATE TABLE entity_type_names (
    id INTEGER PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
    name TEXT COLLATE "C" NOT NULL,
    CONSTRAINT uq_entity_type_names_name UNIQUE (name),
    CONSTRAINT chk_entity_type_names_name_length CHECK (char_length(name) BETWEEN 1 AND 2048)
);
