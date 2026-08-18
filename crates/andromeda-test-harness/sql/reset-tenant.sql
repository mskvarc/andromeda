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
-- that depend on a foreign key rather than on this list.

TRUNCATE
    write_records,
    temporal_append_intents,
    pending_side_effects,
    current_attribute_values,
    notification_deliveries,
    notification_events,
    scope_history,
    snapshot_temporal_entities,
    snapshot_entities,
    snapshots,
    registration_information,
    context_source_registrations,
    subscription_delivery_state,
    subscriptions,
    attribute_history_instance_claims,
    attribute_history,
    temporal_attribute_streams,
    temporal_entities,
    entity_search,
    entity_documents,
    entities,
    jsonld_contexts
RESTART IDENTITY CASCADE
