-- ============================================================================
-- 0020: Discovery census
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 5.2.25, 5.2.26, 5.2.28, 5.7.5-5.7.10
--
-- Signed-delta ledgers that answer entity type and attribute discovery without
-- reading the payloads. They carry no foreign key and share only the dictionary
-- keys the entity relations hold, so they live apart from the entity tables
-- that maintain them.
-- ============================================================================
-- Entity type and attribute discovery reports what the Entities carry now: how
-- many Entities have each type (clause 5.2.26 `entityCount`), the attributes
-- and families each type carries (clauses 5.2.25 and 5.2.26), and how many
-- instances each attribute name has across all Entities (Table 5.2.28-1
-- `attributeCount`: a `datasetId` instance counts, an Entity with two types
-- counts once). Answering that from the payloads reads every Entity.
--
-- The three relations are ledgers of signed changes to those counts. Every
-- statement that inserts, rewrites or deletes `entity_search` rows adds the
-- difference between what it wrote and what it replaced, from its own
-- `RETURNING`, in the same statement, and only when the row changed. A count is
-- the sum of its rows, exact in every snapshot; a writer only ever inserts, so
-- no counter row is shared and none waits on another.
--
-- A periodic fold replaces the rows of a key that has more than one with their
-- sum, so a ledger holds about one row per live key at rest. Correctness never
-- depends on the fold, only the cost of a read.
--
-- No foreign key to the dictionaries: `entity_search` holds the same keys
-- without one, and a key-share lock per written change is not worth it.
-- Entities per type: `entityCount`, and whether a type exists at all.
create table entity_type_census (
    entity_type_name_key integer not null,
    entity_delta bigint not null,
    constraint chk_entity_type_census_entity_delta check (entity_delta <> 0)
)
with (
    autovacuum_vacuum_insert_scale_factor = 0.01,
    autovacuum_vacuum_insert_threshold = 500,
    autovacuum_vacuum_scale_factor = 0.01,
    autovacuum_vacuum_threshold = 200,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 200,
    autovacuum_vacuum_cost_delay = 0
);

-- Attribute instances per type, attribute name and family: the attributes of
-- a type and their `attributeTypes`, and the types of an attribute.
create table entity_type_attribute_census (
    entity_type_name_key integer not null,
    attribute_name_key integer not null,
    attribute_kind smallint not null,
    instance_delta bigint not null,
    constraint chk_entity_type_attribute_census_attribute_kind check (attribute_kind between 1 and 8),
    constraint chk_entity_type_attribute_census_instance_delta check (instance_delta <> 0)
)
with (
    autovacuum_vacuum_insert_scale_factor = 0.01,
    autovacuum_vacuum_insert_threshold = 500,
    autovacuum_vacuum_scale_factor = 0.01,
    autovacuum_vacuum_threshold = 200,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 200,
    autovacuum_vacuum_cost_delay = 0
);

-- Attribute instances per attribute name across every Entity, whatever its
-- types: `attributeCount`, which the per-type ledger cannot give, since an
-- Entity with two types is counted there under each.
create table attribute_census (
    attribute_name_key integer not null,
    instance_delta bigint not null,
    constraint chk_attribute_census_instance_delta check (instance_delta <> 0)
)
with (
    autovacuum_vacuum_insert_scale_factor = 0.01,
    autovacuum_vacuum_insert_threshold = 500,
    autovacuum_vacuum_scale_factor = 0.01,
    autovacuum_vacuum_threshold = 200,
    autovacuum_analyze_scale_factor = 0.02,
    autovacuum_analyze_threshold = 200,
    autovacuum_vacuum_cost_delay = 0
);

