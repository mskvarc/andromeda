-- Every mutable tenant table, truncated between test cases.
--
-- Four tables are deliberately absent. `attribute_names` is a dictionary whose
-- keys are cached in-process, so truncating it would strand every cached key.
-- `schema_metadata` carries the schema contract the broker stamps once after
-- migrating and verifies on every later connection to the tenant: removing the
-- singleton makes the tenant read as never-stamped, and every connection after
-- the reset is refused as drift. `unit_codes` and `unit_code_catalog_metadata`
-- are reference data seeded by a migration from the assignment the broker
-- compiles in: emptying them would leave every unit-code write without the row
-- its foreign key needs, and nothing outside a migration may reseed them.

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
    subscriptions,
    attribute_history,
    temporal_attribute_streams,
    temporal_entities,
    entities,
    jsonld_contexts
RESTART IDENTITY CASCADE
