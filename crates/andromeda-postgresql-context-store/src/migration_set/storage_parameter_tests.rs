//! The vacuum, BRIN and sequence-cache parameters of the hot relations.

use crate::migration_set::declared_schema::DeclaredSchema;
use pretty_assertions::assert_eq;

/// The relations that fill and drain, or hold current state rewritten in
/// place: vacuumed by how many rows changed, never by a fraction of their
/// size, and never truncated at the tail.
const QUEUE_AND_CURRENT_STATE_TABLES: [&str; 14] = [
    "entities",
    "entity_search",
    "entity_type_members",
    "entity_type_census",
    "entity_type_attribute_census",
    "attribute_census",
    "current_attributes",
    "subscription_delivery_state",
    "registration_delivery_state",
    "snapshot_preparation",
    "notification_events",
    "notification_deliveries",
    "pending_side_effects",
    "temporal_append_intents",
];

/// The history tables, appended to and vacuumed for their inserts.
const HISTORY_TABLES: [&str; 2] = ["attribute_history", "scope_history"];

/// The value `table` declares for storage parameter `name`, if any.
fn parameter<'s>(schema: &'s DeclaredSchema, table: &str, name: &str) -> Option<&'s str> {
    schema
        .storage_parameters
        .iter()
        .find(|parameter| parameter.table == table && parameter.name == name)
        .map(|parameter| parameter.value.as_str())
}

#[test]
fn every_queue_and_current_state_table_keeps_its_tail_and_vacuums_by_the_rows_that_changed() {
    let schema = DeclaredSchema::of_every_migration();
    for table in QUEUE_AND_CURRENT_STATE_TABLES {
        assert_eq!(parameter(&schema, table, "vacuum_truncate"), Some("false"), "{table}");
        assert_eq!(parameter(&schema, table, "autovacuum_vacuum_scale_factor"), Some("0"), "{table}");
        let threshold = parameter(&schema, table, "autovacuum_vacuum_threshold").and_then(|threshold| threshold.parse::<u32>().ok());
        assert!(
            threshold.is_some_and(|threshold| threshold > 0),
            "{table} declares a positive vacuum threshold: {threshold:?}"
        );
        if let Some(insert_scale_factor) = parameter(&schema, table, "autovacuum_vacuum_insert_scale_factor") {
            assert_eq!(insert_scale_factor, "0", "{table}");
            assert!(parameter(&schema, table, "autovacuum_vacuum_insert_threshold").is_some(), "{table}");
        }
    }
}

/// Every core write of an Entity stamps its current state's `modifiedAt` and
/// every recording write its Evolution's, both on the one identity row, so the
/// row gets the high-volume profile and room for two heap-only updates.
#[test]
fn the_identity_rows_declare_the_high_volume_profile() {
    let schema = DeclaredSchema::of_every_migration();
    let declared: Vec<(&str, &str)> = schema
        .storage_parameters
        .iter()
        .filter(|parameter| parameter.table == "entities")
        .map(|parameter| (parameter.name.as_str(), parameter.value.as_str()))
        .collect();
    assert_eq!(
        declared,
        [
            ("vacuum_truncate", "false"),
            ("fillfactor", "70"),
            ("autovacuum_vacuum_scale_factor", "0"),
            ("autovacuum_vacuum_threshold", "500"),
            ("autovacuum_analyze_scale_factor", "0.02"),
            ("autovacuum_analyze_threshold", "500"),
            ("autovacuum_vacuum_cost_delay", "0"),
        ]
    );
}

/// Notification events are never updated, only reaped, so their inserts are
/// what leaves pages for a vacuum to mark all-visible.
#[test]
fn notification_events_take_an_insert_driven_vacuum_by_row_count() {
    let schema = DeclaredSchema::of_every_migration();
    assert_eq!(parameter(&schema, "notification_events", "autovacuum_vacuum_insert_scale_factor"), Some("0"));
    assert_eq!(parameter(&schema, "notification_events", "autovacuum_vacuum_insert_threshold"), Some("500"));
}

#[test]
fn the_history_tables_take_an_insert_driven_vacuum_once_a_twentieth_of_their_rows_are_new() {
    let schema = DeclaredSchema::of_every_migration();
    for table in HISTORY_TABLES {
        assert_eq!(parameter(&schema, table, "autovacuum_vacuum_insert_scale_factor"), Some("0.05"), "{table}");
    }
}

#[test]
fn every_history_brin_index_covers_32_pages_and_summarises_as_appends_pass() {
    let schema = DeclaredSchema::of_every_migration();
    let brin: Vec<&str> = schema
        .indexes
        .iter()
        .filter(|index| HISTORY_TABLES.contains(&index.table.as_str()) && index.definition.contains("using brin"))
        .map(|index| {
            assert!(
                index.definition.ends_with("with (pages_per_range = 32, autosummarize = on)"),
                "{}: {}",
                index.name,
                index.definition
            );
            index.name.as_str()
        })
        .collect();
    assert_eq!(brin.len(), 4, "one summary per semantic axis of attribute history: {brin:?}");
}

#[test]
fn the_hot_identities_cache_32_keys_and_the_intent_key_caches_none() {
    let schema = DeclaredSchema::of_every_migration();
    let definition = |table: &str, column: &str| {
        schema
            .columns
            .iter()
            .find(|declared| declared.table == table && declared.name == column)
            .map(|declared| declared.definition.as_str())
    };
    for (table, column) in [
        ("notification_events", "event_key"),
        ("notification_deliveries", "delivery_key"),
        ("attribute_names", "attribute_name_key"),
        ("entity_type_names", "entity_type_name_key"),
    ] {
        assert!(
            definition(table, column).is_some_and(|definition| definition.contains("generated always as identity (cache 32)")),
            "{table}.{column}: {:?}",
            definition(table, column)
        );
    }
    assert!(
        definition("temporal_append_intents", "intent_key").is_some_and(|definition| !definition.contains("cache")),
        "an intent's key orders one Entity's intents by commit"
    );
}
