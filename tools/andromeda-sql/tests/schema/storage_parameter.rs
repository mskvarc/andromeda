//! The vacuum, BRIN and sequence-cache parameters of the hot relations.

use crate::repository::{PinError, parameter, schema, written};
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

#[test]
fn every_queue_and_current_state_table_keeps_its_tail_and_vacuums_by_the_rows_that_changed() -> Result<(), PinError> {
    let schema = schema()?;
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
    Ok(())
}

/// Every core write of an Entity stamps its current state's `modifiedAt` and
/// every recording write its Evolution's, both on the one identity row, so the
/// row gets the high-volume profile and room for two heap-only updates.
#[test]
fn the_identity_rows_declare_the_high_volume_profile() -> Result<(), PinError> {
    let schema = schema()?;
    let declared: Vec<(&str, &str)> = schema
        .storage_parameters()
        .iter()
        .filter(|parameter| parameter.table() == "entities")
        .map(|parameter| (parameter.name(), parameter.value()))
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
    Ok(())
}

/// Notification events are never updated, only reaped, so their inserts are
/// what leaves pages for a vacuum to mark all-visible.
#[test]
fn notification_events_take_an_insert_driven_vacuum_by_row_count() -> Result<(), PinError> {
    let schema = schema()?;
    assert_eq!(parameter(&schema, "notification_events", "autovacuum_vacuum_insert_scale_factor"), Some("0"));
    assert_eq!(parameter(&schema, "notification_events", "autovacuum_vacuum_insert_threshold"), Some("500"));
    Ok(())
}

#[test]
fn the_history_tables_take_an_insert_driven_vacuum_once_a_twentieth_of_their_rows_are_new() -> Result<(), PinError> {
    let schema = schema()?;
    for table in HISTORY_TABLES {
        assert_eq!(parameter(&schema, table, "autovacuum_vacuum_insert_scale_factor"), Some("0.05"), "{table}");
    }
    Ok(())
}

#[test]
fn every_history_brin_index_covers_32_pages_and_summarises_as_appends_pass() -> Result<(), PinError> {
    let schema = schema()?;
    let brin: Vec<&str> = schema
        .indexes()
        .iter()
        .filter(|index| HISTORY_TABLES.contains(&index.table()) && index.method() == "brin")
        .map(|index| {
            let definition = written(&schema, index.origin(), index.definition());
            assert!(
                definition.ends_with("with (pages_per_range = 32, autosummarize = on)"),
                "{}: {definition}",
                index.name()
            );
            index.name()
        })
        .collect();
    assert_eq!(brin.len(), 4, "one summary per semantic axis of attribute history: {brin:?}");
    Ok(())
}

#[test]
fn the_hot_identities_cache_32_keys_and_the_intent_key_caches_none() -> Result<(), PinError> {
    let schema = schema()?;
    let definition = |table: &str, column: &str| {
        schema
            .columns()
            .iter()
            .find(|declared| declared.relation() == table && declared.name() == column)
            .map(|declared| written(&schema, declared.origin(), declared.definition()))
    };
    for (table, column) in [
        ("notification_events", "event_key"),
        ("notification_deliveries", "delivery_key"),
        ("attribute_names", "attribute_name_key"),
        ("entity_type_names", "entity_type_name_key"),
    ] {
        let declared = definition(table, column);
        assert!(
            declared
                .as_deref()
                .is_some_and(|declared| declared.contains("generated always as identity (cache 32)")),
            "{table}.{column}: {declared:?}"
        );
    }
    assert!(
        definition("temporal_append_intents", "intent_key").is_some_and(|declared| !declared.contains("cache")),
        "an intent's key orders one Entity's intents by commit"
    );
    Ok(())
}
