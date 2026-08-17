-- ============================================================================
-- 0007: Attribute name dictionary
-- ============================================================================
-- Normalization layer between attribute IRIs and every relation that carries
-- an attribute identity (temporal streams, current-value projection,
-- registration routing). Instead of repeating a 60+ char IRI on every row of
-- those relations, each stores a compact INTEGER reference into this table.
--
-- `id` is INTEGER rather than SMALLINT: attribute IRIs are open-world domain
-- values and NGSI-LD sets no corresponding limit, so a 32,767-key ceiling
-- would be a capacity accident rather than a modelled constraint.
--
-- `name` is `TEXT COLLATE "C"`. Attribute IRIs are identifiers, not prose --
-- nothing about them has linguistic ordering semantics, every comparison the
-- broker makes on them is equality or an anchored prefix, and they are looked
-- up through a unique btree on every write of a new attribute. `COLLATE "C"`
-- gives that btree byte-wise comparison, makes anchored patterns compile to
-- range scans without a pattern-operator index, and removes the standing
-- hazard of a libc or ICU collation change invalidating the index across an
-- operating-system upgrade.
--
-- The length bound caps what a single hostile name can cost the heap and the
-- unique index; capping the *rate* of new names is an API-layer concern and
-- is not attempted here. 2048 is migration-owned and must equal the bound the
-- request layer enforces before a name reaches this table, so an over-long
-- name is refused with an NGSI-LD error rather than a constraint violation
-- surfacing as a persistence failure.
--
-- The dictionary stays append-only and therefore open to inflation: any
-- client can mint unbounded distinct attribute IRIs. Rate limiting new-name
-- creation and observing dictionary growth are API-layer concerns and are
-- deliberately absent from this migration.
-- ============================================================================

CREATE TABLE attribute_names (
    id INTEGER PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
    name TEXT COLLATE "C" NOT NULL,
    CONSTRAINT uq_attribute_names_name UNIQUE (name),
    CONSTRAINT chk_attribute_names_name_length CHECK (char_length(name) BETWEEN 1 AND 2048)
);
