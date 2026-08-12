-- ============================================================================
-- 0064: The routing projection of a registration's information
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
-- derives it and rewrites it inside the same transaction as every registration
-- mutation (`information_projection.rs`). Nothing in the database derives it.
-- There is deliberately no trigger parsing `information` -- that is Scorpio's
-- `csourceinformation` design, and it puts clause-4.17 and clause-5.2.9
-- semantics into PL/pgSQL where no test, no type and no reviewer of the broker
-- ever sees them.
--
-- The projection decides nothing. It preselects, and the typed evaluation that
-- follows -- the `information` unnesting for discovery, the Rust matching for
-- forwarding -- remains the oracle for membership. Every derivation choice
-- therefore resolves ambiguity towards the wider row: an over-admitted
-- registration costs one residual test, an omitted one silently disappears from
-- distributed operations.
--
-- A registration that constrains nothing has *no rows here at all*. Information
-- items are disjunctive, so an item that bounds no axis -- and equally an empty
-- `information` -- makes the whole registration a candidate for every request.
-- That is what lets `chk_registration_information_selector_present` hold on
-- every row that does exist, and it is why every reader disjoins its dimension
-- test with `NOT EXISTS (SELECT 1 FROM registration_information ...)`. Reading
-- this table as an inner join would drop exactly the registrations that serve
-- everything.
--
-- What the columns hold, and why they are typed as they are:
--
--   * `registration_id` is the registration's own URN. There is no surrogate
--     key on `context_source_registrations`: registrations are catalogue-scale
--     and each yields a handful of rows, so interning the URN would buy no
--     measurable width while forcing an extra round trip into every mutation.
--     `COLLATE "C"` for the reason 0056 and 0061 record -- these are
--     identifiers, compared for equality, and byte-wise comparison keeps the
--     btree free of any locale.
--   * `entity_types` is the set of type names a selector's clause-4.17
--     expression mentions, or NULL where the expression admits every type. A
--     conjunction is stored as its members, which is wider than the expression
--     and therefore permitted; the typed expression still decides. Readers ask
--     `entity_types && ARRAY[...]`, which is what the GIN index below serves.
--   * `entity_id` and `entity_id_pattern` stay text. An identifier is matched
--     for equality and a pattern is compared as written -- never evaluated as a
--     regular expression here, because PostgreSQL's dialect is not the one
--     clause 4.9 mandates and a dialect disagreement would drop candidates
--     rather than merely over-admit them. Neither is a closed vocabulary worth
--     a dictionary.
--   * `property_name_ids` and `relationship_name_ids` are the four-byte
--     `attribute_names` keys 0035 widened, not expanded IRIs -- the same keys
--     every other relation in this schema indexes attribute names by. NULL on
--     both columns means the item declared no name at all; the writer widens
--     the two together, so one being NULL beside a populated one always means
--     that axis was genuinely undeclared.
--   * `operations` is the registration-wide clause-5.2.9 member, replicated
--     onto every row of that registration so one indexed row carries every
--     dimension a preselect could test. It holds the numbers of the
--     migration-owned registry the broker compiles in
--     (`andromeda-postgresql-codes` `CsrOperationCode`, 1 `createEntity`
--     through 48 `redirectionOps`, the five Table 4.20-2 group tokens included
--     as members in their own right), not the clause-4.20 tokens as text. The
--     check pins the range: a number outside it could only come from a release
--     that knows an operation this schema does not.
--
--     An array rather than a bitmask, because the question asked of it is
--     "does this registration declare operation X, or a group covering it",
--     which is `&&` -- an operator GIN serves directly -- and because a
--     forty-eight-member vocabulary that already grew once must not be capped
--     at the sixty-four a bitmask word would allow. No index on it yet: no
--     reader filters on operations today (`CompiledCsrQuery` carries no
--     operations filter and `RegistrationStore::find_matching` takes no
--     operations argument), operation matching stays residual, and an index
--     with no query behind it is maintenance cost for nothing.
--
-- The index set is exactly the predicates the readers emit
-- (`andromeda-postgresql-context-query` `csr::projection`), one named consumer
-- each: `registration_id` for the correlation and the emptiness test that every
-- dimension disjoins with, and for the delete half of the writer's wholesale
-- rewrite; GIN on `entity_types`, `property_name_ids` and
-- `relationship_name_ids` for the three overlap tests; partial btrees on
-- `entity_id` and `entity_id_pattern` for the two identifier tests, partial
-- because a row that constrains neither is admitted without being looked up.
--
-- ---------------------------------------------------------------------------
-- Backfill
-- ---------------------------------------------------------------------------
-- Set-based, from the documents themselves, in this file. Three points where
-- SQL cannot reproduce the broker's derivation exactly, each resolved towards
-- the wider row:
--
--   * A clause-4.17 type expression is a grammar, and this file does not parse
--     one. A `type` that contains any of the expression characters `* ( ) ; ,
--     |` widens the row's type axis to NULL -- unconstrained -- instead of
--     being stored as a name no request will ever equal. A leaf name, which is
--     what the overwhelming majority of registrations carry, is stored as
--     written. Widening is the safe direction and needs no reconcile.
--   * A declared attribute name that the dictionary does not yet hold would
--     widen the whole attribute constraint away, which is correct but lossy.
--     The first statement below interns those names instead, so the backfilled
--     rows carry the same keys the broker would mint on the next write. The
--     dictionary is append-only and this adds at most one row per distinct
--     registered attribute IRI.
--   * An operation token outside the registry cannot be mapped at all. It is
--     asserted rather than dropped: the cast in the mapping below fails the
--     migration and names the registration, because a token this schema cannot
--     encode is a registration the broker could not have accepted.
--
-- The assertions after the insert check the two directions the derivation could
-- go wrong in: a registration that projected rows must have projected exactly
-- one per (information item, entity selector) pair, and a registration that
-- projected none must have an information item that genuinely constrains
-- nothing.
-- ============================================================================

CREATE TABLE registration_information (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    registration_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL
        REFERENCES context_source_registrations(id) ON DELETE CASCADE,
    entity_types TEXT[] COMPRESSION lz4 COLLATE "C",
    entity_id TEXT COMPRESSION lz4 COLLATE "C",
    entity_id_pattern TEXT COMPRESSION lz4,
    property_name_ids INTEGER[] COMPRESSION lz4,
    relationship_name_ids INTEGER[] COMPRESSION lz4,
    operations SMALLINT[] COMPRESSION lz4,
    CONSTRAINT chk_registration_information_selector_present CHECK (
        entity_types IS NOT NULL
        OR entity_id IS NOT NULL
        OR entity_id_pattern IS NOT NULL
        OR property_name_ids IS NOT NULL
        OR relationship_name_ids IS NOT NULL),
    CONSTRAINT chk_registration_information_entity_types_populated CHECK (
        entity_types IS NULL
        OR (cardinality(entity_types) > 0 AND array_position(entity_types, NULL) IS NULL)),
    CONSTRAINT chk_registration_information_property_name_ids_populated CHECK (
        property_name_ids IS NULL
        OR (cardinality(property_name_ids) > 0
            AND array_position(property_name_ids, NULL) IS NULL
            AND 0 < ALL (property_name_ids))),
    CONSTRAINT chk_registration_information_relationship_name_ids_populated CHECK (
        relationship_name_ids IS NULL
        OR (cardinality(relationship_name_ids) > 0
            AND array_position(relationship_name_ids, NULL) IS NULL
            AND 0 < ALL (relationship_name_ids))),
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

-- Intern every registered attribute name the dictionary does not hold yet, so
-- the backfill below can carry keys rather than widening the axis away. The
-- `jsonb_typeof` guard is what keeps a member that is present but not an array
-- -- which the typed writer cannot produce, but an older document might carry
-- -- from aborting the migration inside `jsonb_array_elements_text`.
INSERT INTO attribute_names (name)
SELECT DISTINCT declared.name
FROM context_source_registrations AS r
CROSS JOIN LATERAL jsonb_array_elements(r.information) AS item(entry)
CROSS JOIN LATERAL (
    SELECT value AS name
    FROM jsonb_array_elements_text(
        CASE WHEN jsonb_typeof(item.entry -> 'propertyNames') = 'array'
             THEN item.entry -> 'propertyNames' ELSE '[]'::JSONB END)
    UNION
    SELECT value AS name
    FROM jsonb_array_elements_text(
        CASE WHEN jsonb_typeof(item.entry -> 'relationshipNames') = 'array'
             THEN item.entry -> 'relationshipNames' ELSE '[]'::JSONB END)
) AS declared
WHERE NOT EXISTS (SELECT 1 FROM attribute_names AS a WHERE a.name = declared.name);

INSERT INTO registration_information (
    registration_id, entity_types, entity_id, entity_id_pattern,
    property_name_ids, relationship_name_ids, operations)
WITH registration_operations AS (
    -- The registry mapping is migration-owned, and it appears once: an
    -- unregistered token reaches the `COALESCE` fallback, whose cast to
    -- SMALLINT fails the migration with the offending registration named.
    -- COALESCE evaluates its second argument only when the first is NULL, which
    -- is exactly when the left join found no registry member.
    SELECT r.id AS registration_id,
           CASE WHEN r.operations IS NULL OR cardinality(r.operations) = 0 THEN NULL
                ELSE (
                    SELECT array_agg(COALESCE(
                               registry.code,
                               CAST('context source registration ' || r.id
                                    || ' declares operation ' || declared.token
                                    || ', which the code registry does not name' AS SMALLINT))
                           ORDER BY registry.code)
                    FROM unnest(r.operations) AS declared(token)
                    LEFT JOIN (VALUES
                        ('createEntity', 1::SMALLINT),
                        ('updateEntity', 2),
                        ('appendAttrs', 3),
                        ('updateAttrs', 4),
                        ('deleteAttrs', 5),
                        ('deleteEntity', 6),
                        ('createBatch', 7),
                        ('upsertBatch', 8),
                        ('updateBatch', 9),
                        ('deleteBatch', 10),
                        ('upsertTemporal', 11),
                        ('appendAttrsTemporal', 12),
                        ('deleteAttrsTemporal', 13),
                        ('updateAttrInstanceTemporal', 14),
                        ('deleteAttrInstanceTemporal', 15),
                        ('deleteTemporal', 16),
                        ('mergeEntity', 17),
                        ('replaceEntity', 18),
                        ('replaceAttrs', 19),
                        ('mergeBatch', 20),
                        ('purgeEntity', 21),
                        ('retrieveEntity', 22),
                        ('queryEntity', 23),
                        ('queryBatch', 24),
                        ('retrieveTemporal', 25),
                        ('queryTemporal', 26),
                        ('retrieveEntityTypes', 27),
                        ('retrieveEntityTypeDetails', 28),
                        ('retrieveEntityTypeInfo', 29),
                        ('retrieveAttrTypes', 30),
                        ('retrieveAttrTypeDetails', 31),
                        ('retrieveAttrTypeInfo', 32),
                        ('createSubscription', 33),
                        ('updateSubscription', 34),
                        ('retrieveSubscription', 35),
                        ('querySubscription', 36),
                        ('deleteSubscription', 37),
                        ('retrieveEntityMap', 38),
                        ('updateEntityMap', 39),
                        ('deleteEntityMap', 40),
                        ('createEntityMapQueryEntity', 41),
                        ('createEntityMapQueryTemporal', 42),
                        ('retrieveContextSourceIdentity', 43),
                        ('federationOps', 44),
                        ('associationOps', 45),
                        ('updateOps', 46),
                        ('retrieveOps', 47),
                        ('redirectionOps', 48)
                    ) AS registry(label, code) ON registry.label = declared.token
                ) END AS operations
    FROM context_source_registrations AS r
),
entry_names AS (
    SELECT r.id AS registration_id,
           item.entry,
           (SELECT array_agg(a.id ORDER BY a.id)
            FROM attribute_names AS a
            WHERE a.name IN (
                SELECT value FROM jsonb_array_elements_text(
                    CASE WHEN jsonb_typeof(item.entry -> 'propertyNames') = 'array'
                         THEN item.entry -> 'propertyNames' ELSE '[]'::JSONB END))) AS property_name_ids,
           (SELECT array_agg(a.id ORDER BY a.id)
            FROM attribute_names AS a
            WHERE a.name IN (
                SELECT value FROM jsonb_array_elements_text(
                    CASE WHEN jsonb_typeof(item.entry -> 'relationshipNames') = 'array'
                         THEN item.entry -> 'relationshipNames' ELSE '[]'::JSONB END))) AS relationship_name_ids
    FROM context_source_registrations AS r
    CROSS JOIN LATERAL jsonb_array_elements(r.information) AS item(entry)
),
derived AS (
    -- One row per (information item, entity selector). The left join is what
    -- gives an item with no `entities`, or an empty one, its single row with
    -- every entity dimension unconstrained.
    SELECT n.registration_id,
           CASE WHEN selector.value ->> 'type' IS NULL THEN NULL
                WHEN selector.value ->> 'type' ~ '[*(),;|]' THEN NULL
                ELSE ARRAY[selector.value ->> 'type'] END AS entity_types,
           selector.value ->> 'id' AS entity_id,
           selector.value ->> 'idPattern' AS entity_id_pattern,
           n.property_name_ids,
           n.relationship_name_ids
    FROM entry_names AS n
    LEFT JOIN LATERAL jsonb_array_elements(
        CASE WHEN jsonb_typeof(n.entry -> 'entities') = 'array'
             THEN n.entry -> 'entities' ELSE '[]'::JSONB END) AS selector(value) ON TRUE
),
unconstrained AS (
    -- A derived row that bounds no axis is an information item that bounds
    -- nothing, and items are disjunctive: the registration serves everything and
    -- keeps no rows at all.
    SELECT DISTINCT d.registration_id
    FROM derived AS d
    WHERE d.entity_types IS NULL
      AND d.entity_id IS NULL
      AND d.entity_id_pattern IS NULL
      AND d.property_name_ids IS NULL
      AND d.relationship_name_ids IS NULL
)
SELECT d.registration_id, d.entity_types, d.entity_id, d.entity_id_pattern,
       d.property_name_ids, d.relationship_name_ids, o.operations
FROM derived AS d
JOIN registration_operations AS o ON o.registration_id = d.registration_id
WHERE NOT EXISTS (SELECT 1 FROM unconstrained AS u WHERE u.registration_id = d.registration_id);

-- A registration that projected anything projected one row per (information
-- item, entity selector) pair. The pair count is recomputed here from the
-- document alone, without the derivation above, so a dropped or duplicated row
-- fails the migration rather than silently narrowing discovery.
SELECT CAST('context source registration ' || counted.registration_id
            || ' projected ' || counted.projected::TEXT
            || ' routing rows for ' || counted.pairs::TEXT
            || ' information item and entity selector pairs' AS SMALLINT)
FROM (
    SELECT r.id AS registration_id,
           (SELECT count(*) FROM registration_information AS ri WHERE ri.registration_id = r.id) AS projected,
           (SELECT COALESCE(sum(GREATEST(
                       CASE WHEN jsonb_typeof(item.entry -> 'entities') = 'array'
                            THEN jsonb_array_length(item.entry -> 'entities') ELSE 0 END, 1)), 0)
            FROM jsonb_array_elements(r.information) AS item(entry)) AS pairs
    FROM context_source_registrations AS r
) AS counted
WHERE counted.projected <> 0 AND counted.projected <> counted.pairs
LIMIT 1;

-- A registration that projected nothing must have an information item that
-- constrains nothing: none at all, or one declaring no attribute name and
-- carrying either no entity selector or a selector that bounds neither the type
-- axis nor the identifier axis.
SELECT CAST('context source registration ' || r.id
            || ' projected no routing rows although every information item constrains a dimension' AS SMALLINT)
FROM context_source_registrations AS r
WHERE jsonb_array_length(r.information) > 0
  AND NOT EXISTS (SELECT 1 FROM registration_information AS ri WHERE ri.registration_id = r.id)
  AND NOT EXISTS (
      SELECT 1
      FROM jsonb_array_elements(r.information) AS item(entry)
      WHERE COALESCE(jsonb_array_length(
                CASE WHEN jsonb_typeof(item.entry -> 'propertyNames') = 'array'
                     THEN item.entry -> 'propertyNames' ELSE '[]'::JSONB END), 0) = 0
        AND COALESCE(jsonb_array_length(
                CASE WHEN jsonb_typeof(item.entry -> 'relationshipNames') = 'array'
                     THEN item.entry -> 'relationshipNames' ELSE '[]'::JSONB END), 0) = 0
        AND (COALESCE(jsonb_array_length(
                 CASE WHEN jsonb_typeof(item.entry -> 'entities') = 'array'
                      THEN item.entry -> 'entities' ELSE '[]'::JSONB END), 0) = 0
             OR EXISTS (
                 SELECT 1
                 FROM jsonb_array_elements(item.entry -> 'entities') AS selector(value)
                 WHERE selector.value ->> 'id' IS NULL
                   AND selector.value ->> 'idPattern' IS NULL
                   AND (selector.value ->> 'type' IS NULL OR selector.value ->> 'type' ~ '[*(),;|]'))))
LIMIT 1;

CREATE INDEX idx_registration_information_registration
    ON registration_information (registration_id);

CREATE INDEX idx_registration_information_entity_types
    ON registration_information USING GIN (entity_types);

CREATE INDEX idx_registration_information_property_name_ids
    ON registration_information USING GIN (property_name_ids);

CREATE INDEX idx_registration_information_relationship_name_ids
    ON registration_information USING GIN (relationship_name_ids);

CREATE INDEX idx_registration_information_entity_id
    ON registration_information (entity_id)
    WHERE entity_id IS NOT NULL;

CREATE INDEX idx_registration_information_entity_id_pattern
    ON registration_information (entity_id_pattern)
    WHERE entity_id_pattern IS NOT NULL;

ANALYZE registration_information;
