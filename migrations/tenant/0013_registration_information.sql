-- ============================================================================
-- 0013: Registration routing projection
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 4.17, 5.2.8, 5.2.9, 5.10, 5.12
--
-- `context_source_registrations.information` is one JSONB document per
-- registration, because it is rendered back verbatim. Discovery (clause 5.10)
-- and the forwarding coverage decision (clause 5.12) ask the opposite
-- question, which registrations could serve a given entity type, identifier or
-- attribute, and no ordinary index answers that over a nested array of
-- objects. `registration_information` is the document flattened into the
-- shape those predicates filter on: one row per information item per entity
-- selector. The broker rewrites it in the same transaction as every
-- registration mutation; nothing in the database derives it.
--
-- The projection only preselects. The evaluation that follows remains the
-- judge of membership, so every derivation resolves ambiguity towards the
-- wider row: an over-admitted registration costs one residual test, while an
-- omitted one silently drops out of distributed operations.
--
-- A registration that constrains nothing is one wildcard row: the type set
-- `{'*'}`, the identifier `''`, no pattern, and `{0}` on both attribute axes.
-- Information items are disjunctive, so an item that bounds no axis, or an
-- empty `information`, makes the registration a candidate for every request.
-- The wildcard row lets a reader say that with an ordinary indexed test
-- instead of a `NOT EXISTS` disjunct on every dimension. Every column is
-- therefore `NOT NULL` and has a sentinel for "unconstrained on this axis",
-- and every reader predicate names the sentinel beside the requested value.
--
-- The registration's `operations` are not projected: no preselect filters on
-- them.
-- ============================================================================

CREATE TABLE registration_information (
    information_key BIGINT GENERATED ALWAYS AS IDENTITY,
    registration_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    -- The type names in a selector's Table 5.2.8-1 `type` member, or `{'*'}`
    -- where it admits every type (`*` is not a legal type name). A conjunction
    -- is stored as its members, which is wider than the selector and so
    -- permitted. Readers ask `entity_types && ARRAY[$1, '*']`.
    entity_types TEXT[] COMPRESSION lz4 COLLATE "C" NOT NULL,
    -- The exact identifier a selector names, or `''` where it names none (an
    -- identifier is a URI and never empty).
    entity_id TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    -- Compared as written and never evaluated here: PostgreSQL's regular
    -- expression dialect is not the one clause 5.2.8 mandates, and a dialect
    -- disagreement would drop candidates instead of over-admitting them.
    -- Readers ask `entity_id IN ($1, '') OR entity_id_pattern IS NOT NULL`.
    entity_id_pattern TEXT COMPRESSION lz4,
    -- `attribute_names` keys. `{0}` is the sentinel for "declared no name on
    -- either axis"; an item that declared names on one axis only stores `{}`
    -- on the other, which overlaps nothing. The sentinel is set on both axes
    -- together, because the writer widens both when it cannot intern a
    -- declared name. Readers ask
    -- `property_name_keys && (ARRAY(<resolved keys>) || 0)`.
    property_name_keys INTEGER[] COMPRESSION lz4 NOT NULL,
    relationship_name_keys INTEGER[] COMPRESSION lz4 NOT NULL,
    CONSTRAINT pk_registration_information PRIMARY KEY (information_key),
    CONSTRAINT fk_registration_information_registration FOREIGN KEY (registration_id)
        REFERENCES context_source_registrations(id) ON DELETE CASCADE,
    CONSTRAINT chk_registration_information_registration_id_length CHECK (octet_length(registration_id) <= 1024),
    CONSTRAINT chk_registration_information_entity_id_length CHECK (octet_length(entity_id) <= 1024),
    CONSTRAINT chk_registration_information_entity_id_pattern_length CHECK (octet_length(entity_id_pattern) <= 1024),
    CONSTRAINT chk_registration_information_wildcards_well_formed CHECK (
        cardinality(entity_types) > 0
        AND array_position(entity_types, NULL) IS NULL
        AND ('*' <> ALL (entity_types) OR entity_types = ARRAY['*'])
        AND array_position(property_name_keys, NULL) IS NULL
        AND 0 <= ALL (property_name_keys)
        AND (0 <> ALL (property_name_keys) OR property_name_keys = ARRAY[0])
        AND array_position(relationship_name_keys, NULL) IS NULL
        AND 0 <= ALL (relationship_name_keys)
        AND (0 <> ALL (relationship_name_keys) OR relationship_name_keys = ARRAY[0])
        AND ((property_name_keys = ARRAY[0]) = (relationship_name_keys = ARRAY[0])))
) WITH (
    autovacuum_vacuum_scale_factor = 0.05,
    autovacuum_vacuum_threshold = 200,
    autovacuum_analyze_scale_factor = 0.05,
    autovacuum_analyze_threshold = 200
);

-- The correlation with the registration, and the delete half of the writer's
-- wholesale rewrite.
CREATE INDEX idx_registration_information_registration
    ON registration_information (registration_id);

CREATE INDEX idx_registration_information_entity_types
    ON registration_information USING GIN (entity_types);

CREATE INDEX idx_registration_information_property_name_keys
    ON registration_information USING GIN (property_name_keys);

CREATE INDEX idx_registration_information_relationship_name_keys
    ON registration_information USING GIN (relationship_name_keys);

-- Not partial: the sentinel `''` is one of the two values every identifier
-- test names.
CREATE INDEX idx_registration_information_entity_id
    ON registration_information (entity_id);

-- The pattern-presence branch of the identifier test.
CREATE INDEX idx_registration_information_entity_id_pattern
    ON registration_information (entity_id_pattern)
    WHERE entity_id_pattern IS NOT NULL;
