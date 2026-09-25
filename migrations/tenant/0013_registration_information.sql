-- ============================================================================
-- 0013: The routing projection of a registration's information
-- ============================================================================
-- `context_source_registrations.information` (clause 5.2.9) is one JSONB
-- document per registration because that document is the authority for every
-- response body: it is rendered back verbatim and nothing below reduces it.
-- Discovery (clause 5.9) and the forwarding coverage decision (clause 5.10.2)
-- ask the opposite question -- which registrations could serve this entity
-- type, identifier or attribute -- and no ordinary index answers it over a
-- nested array of objects, so every one of those predicates unnests the
-- document once per candidate row.
--
-- `registration_information` is that document flattened into the shape the
-- predicates actually filter on: one row per information item per entity
-- selector, each row carrying every dimension a preselect tests. The broker
-- derives it and rewrites it inside the same transaction as every
-- registration mutation (`information_projection.rs`). Nothing in the
-- database derives it.
--
-- The projection decides nothing. It preselects, and the typed evaluation
-- that follows -- the `information` unnesting for discovery, the Rust
-- matching for forwarding -- remains the oracle for membership. Every
-- derivation choice therefore resolves ambiguity towards the wider row: an
-- over-admitted registration costs one residual test, an omitted one
-- silently disappears from distributed operations.
--
-- A registration that constrains nothing is stored as *one wildcard row*: the
-- type set `{'*'}`, the identifier `''`, no pattern, and the sentinel key `{0}`
-- on both attribute axes. Information items are disjunctive, so an item that
-- bounds no axis -- and equally an empty `information` -- makes the whole
-- registration a candidate for every request, and the wildcard row is what
-- lets a reader express that as an ordinary indexed test instead of the
-- `NOT EXISTS (SELECT 1 FROM registration_information ...)` disjunct an
-- absent row would force on every dimension. Every column below is therefore
-- `NOT NULL`, and each carries a sentinel that stands for "unconstrained on
-- this axis": a reader's predicate is a single overlap or membership test that
-- names the sentinel beside the requested value, one index behind each.
--
-- What the columns hold, and why they are typed as they are:
--
--   * `registration_id` is the registration's own URN. There is no surrogate
--     key on `context_source_registrations`: registrations are
--     catalogue-scale and each yields a handful of rows, so interning the
--     URN would buy no measurable width while forcing an extra round trip
--     into every mutation. `COLLATE "C"` -- these are identifiers, compared
--     for equality, and byte-wise comparison keeps the btree free of any
--     locale.
--   * `entity_types` is the set of type names a selector's clause-4.17
--     expression mentions, or `{'*'}` where the expression admits every type
--     (`*` is not a legal type name, so the sentinel cannot collide). A
--     conjunction is stored as its members, which is wider than the expression
--     and therefore permitted; the typed expression still decides. Readers ask
--     `entity_types && ARRAY[$1, '*']`, which is what the GIN index below
--     serves.
--   * `entity_id` is the exact identifier a selector names, or `''` where it
--     names none (an identifier is a URI and never empty). `entity_id_pattern`
--     stays nullable text: a pattern is compared as written -- never evaluated
--     as a regular expression here, because PostgreSQL's dialect is not the one
--     clause 4.9 mandates and a dialect disagreement would drop candidates
--     rather than merely over-admit them. Readers ask
--     `entity_id IN ($1, '') OR entity_id_pattern IS NOT NULL`.
--   * `property_name_ids` and `relationship_name_ids` are the four-byte
--     `attribute_names` keys, not expanded IRIs -- the same keys every other
--     relation in this schema indexes attribute names by. Dictionary keys start
--     at 1, so `{0}` is the sentinel for "declared no name on either axis";
--     an item that declared names on one axis only stores `{}` on the other,
--     which overlaps nothing. The sentinel is always set on both axes together
--     because the writer widens both together when it cannot intern a declared
--     name; the well-formedness check pins that. Readers ask
--     `property_name_ids && (ARRAY(<resolved keys>) || 0)`.
--   * `operations` is the registration-wide clause-5.2.9 member, replicated
--     onto every row of that registration so one indexed row carries every
--     dimension a preselect could test. It holds the numbers of the
--     migration-owned registry the broker compiles in
--     (`andromeda-postgresql-codes` `CsrOperationCode`, 1 `createEntity`
--     through 48 `redirectionOps`, the five Table 4.20-2 group tokens
--     included as members in their own right), not the clause-4.20 tokens as
--     text -- the encoding the registration's own `operations` column (0012)
--     carries, under the same check. The check pins the range: a number
--     outside it could only come from a release that knows an operation this
--     schema does not.
--
--     An array rather than a bitmask, because the question asked of it is
--     "does this registration declare operation X, or a group covering it",
--     which is `&&` -- an operator GIN serves directly -- and because a
--     forty-eight-member vocabulary that already grew once must not be
--     capped at the sixty-four a bitmask word would allow. No index on it
--     yet: no reader filters on operations today, operation matching stays
--     residual, and an index with no query behind it is maintenance cost for
--     nothing.
--
-- The index set is exactly the predicates the readers emit
-- (`andromeda-postgresql-context-query` `csr::projection`), one named
-- consumer each: `registration_id` for the correlation and for the delete
-- half of the writer's wholesale rewrite; GIN on `entity_types`,
-- `property_name_ids` and `relationship_name_ids` for the three overlap
-- tests; a btree on `entity_id` for the identifier membership test (not
-- partial: the sentinel `''` is one of the two values every such test names)
-- and a partial btree on `entity_id_pattern` for the pattern-presence branch.
-- ============================================================================

CREATE TABLE registration_information (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    registration_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL
        REFERENCES context_source_registrations(id) ON DELETE CASCADE,
    entity_types TEXT[] COMPRESSION lz4 COLLATE "C" NOT NULL,
    entity_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    entity_id_pattern TEXT COMPRESSION lz4,
    property_name_ids INTEGER[] COMPRESSION lz4 NOT NULL,
    relationship_name_ids INTEGER[] COMPRESSION lz4 NOT NULL,
    operations SMALLINT[] COMPRESSION lz4,
    CONSTRAINT chk_registration_information_wildcards_well_formed CHECK (
        cardinality(entity_types) > 0
        AND array_position(entity_types, NULL) IS NULL
        AND ('*' <> ALL (entity_types) OR entity_types = ARRAY['*'])
        AND array_position(property_name_ids, NULL) IS NULL
        AND 0 <= ALL (property_name_ids)
        AND (0 <> ALL (property_name_ids) OR property_name_ids = ARRAY[0])
        AND array_position(relationship_name_ids, NULL) IS NULL
        AND 0 <= ALL (relationship_name_ids)
        AND (0 <> ALL (relationship_name_ids) OR relationship_name_ids = ARRAY[0])
        AND ((property_name_ids = ARRAY[0]) = (relationship_name_ids = ARRAY[0]))),
    CONSTRAINT chk_registration_information_operations_registered CHECK (
        operations IS NULL
        OR (cardinality(operations) > 0
            AND array_position(operations, NULL) IS NULL
            AND 1 <= ALL (operations)
            AND 48 >= ALL (operations)))
) WITH (
    autovacuum_vacuum_scale_factor = 0.05,
    autovacuum_vacuum_threshold = 200,
    autovacuum_analyze_scale_factor = 0.05,
    autovacuum_analyze_threshold = 200
);

CREATE INDEX idx_registration_information_registration
    ON registration_information (registration_id);

CREATE INDEX idx_registration_information_entity_types
    ON registration_information USING GIN (entity_types);

CREATE INDEX idx_registration_information_property_name_ids
    ON registration_information USING GIN (property_name_ids);

CREATE INDEX idx_registration_information_relationship_name_ids
    ON registration_information USING GIN (relationship_name_ids);

CREATE INDEX idx_registration_information_entity_id
    ON registration_information (entity_id);

CREATE INDEX idx_registration_information_entity_id_pattern
    ON registration_information (entity_id_pattern)
    WHERE entity_id_pattern IS NOT NULL;
