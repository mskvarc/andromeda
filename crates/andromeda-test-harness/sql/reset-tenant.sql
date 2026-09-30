-- Every mutable tenant table, truncated between test cases.
--
-- Five tables are deliberately absent. `attribute_names` and
-- `entity_type_names` are dictionaries whose keys are cached in-process, so
-- truncating either would strand every cached key.
-- `schema_metadata` carries the schema contract the broker stamps once after
-- migrating and verifies on every later connection to the tenant: removing the
-- singleton makes the tenant read as never-stamped, and every connection after
-- the reset is refused as drift. `unit_codes` and `unit_code_catalog_metadata`
-- are reference data seeded by a migration from the assignment the broker
-- compiles in: emptying them would leave every unit-code write without the row
-- its foreign key needs, and nothing outside a migration may reseed them.
--
-- `attribute_history_instance_claims` is named explicitly rather than left to
-- the cascade from `temporal_entities`. It carries the clause 4.5.7 uniqueness
-- of a client-supplied `instanceId`, which `attribute_history` cannot carry
-- itself, so a claim surviving the reset would refuse an identifier the next
-- test case is entitled to use -- and relying on a cascade to reclaim it makes
-- that depend on a foreign key rather than on this list. The same holds for
-- `registration_delivery_state`, `subscription_companions`, `forwarded_subscriptions`,
-- `subscription_notified_registrations`, `snapshot_notification_outbox`,
-- `entity_map_entries` and `entity_map_links`:
-- each is also reclaimed by a cascade from
-- the row it hangs off, and each is named here so the reset does not depend on
-- that edge.

-- The whole reset is one statement, so every lock it takes is held until it
-- commits, and it takes all of them before it changes anything: the reset
-- relations, every relation `TRUNCATE ... CASCADE` reaches from them through a
-- foreign key, and every table of every Snapshot branch schema, in one
-- `LOCK TABLE ... NOWAIT`. A broker transaction still open on any of them makes
-- the statement fail at once with `lock_not_available` (55P03) while holding
-- nothing, and the harness runs it again once that transaction has finished.
-- The reset therefore never waits for a lock while holding another, so it can
-- never form a deadlock with a broker's expiry sweep or notification drain --
-- and never makes PostgreSQL abort a broker transaction as the victim of one.
--
-- The live branch is the default schema, reset by the truncation. Each
-- Snapshot branch is its own schema `snap_<key>` (and `snap_<key>_stg` while
-- preparing), test-owned and unreachable from the control tables, so every such
-- schema is dropped after the control rows that named them are gone.

DO $$
DECLARE
    reset_relations CONSTANT regclass[] := ARRAY[
        'write_records',
        'temporal_append_intents',
        'pending_side_effects',
        'current_attribute_values',
        'notification_deliveries',
        'notification_events',
        'scope_history',
        'snapshot_notification_outbox',
        'snapshot_preparation',
        'snapshots',
        'forwarded_subscriptions',
        'subscription_companions',
        'registration_information',
        'registration_delivery_state',
        'context_source_registrations',
        'subscription_notified_registrations',
        'subscription_delivery_state',
        'subscriptions',
        'attribute_history_instance_claims',
        'attribute_history',
        'temporal_attribute_streams',
        'temporal_entities',
        'entity_type_members',
        'entity_type_census',
        'entity_type_attribute_census',
        'attribute_census',
        'entity_search',
        'entity_documents',
        'entities',
        'entity_map_links',
        'entity_map_entries',
        'entity_maps',
        'jsonld_contexts'
    ]::regclass[];
    locked text;
    branch text;
BEGIN
    WITH RECURSIVE reached(relation) AS (
        SELECT unnest(reset_relations)::oid
        UNION
        SELECT constraint_row.conrelid
        FROM pg_constraint AS constraint_row
        JOIN reached ON constraint_row.confrelid = reached.relation
        WHERE constraint_row.contype = 'f'
    ),
    branch_tables(relation) AS (
        SELECT class_row.oid
        FROM pg_class AS class_row
        JOIN pg_namespace AS namespace_row ON namespace_row.oid = class_row.relnamespace
        WHERE namespace_row.nspname LIKE 'snap\_%' AND class_row.relkind IN ('r', 'p')
    )
    SELECT string_agg(relation::regclass::text, ', ')
    INTO locked
    FROM (SELECT relation FROM reached UNION SELECT relation FROM branch_tables) AS every_relation;

    EXECUTE format('LOCK TABLE %s IN ACCESS EXCLUSIVE MODE NOWAIT', locked);
    EXECUTE format('TRUNCATE %s RESTART IDENTITY CASCADE', array_to_string(reset_relations, ', '));
    FOR branch IN SELECT schema_name FROM information_schema.schemata WHERE schema_name LIKE 'snap\_%' LOOP
        EXECUTE format('DROP SCHEMA %I CASCADE', branch);
    END LOOP;
END $$;
