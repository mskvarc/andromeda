//! The byte bounds the migrations state, tied to the Rust constants the broker
//! enforces them with.
//!
//! A bound stated twice -- once as a CHECK a migration declares and once as the
//! constant a parser refuses input with -- drifts the moment one side changes
//! alone. These tests read the embedded migration text, so a migration whose
//! literal disagrees with the constant fails the build rather than surfacing as
//! a constraint violation on a request the parser admitted.

use crate::migration_set::declared_schema::every_migration_sql;
use andromeda_ngsi_ld_lexicon::identifier_length::IDENTIFIER_BYTE_LIMIT;
use andromeda_postgresql_schema::{projected_string_column::INDEXED_STRING_BYTE_LIMIT, projected_uri_column::INDEXED_URI_BYTE_LIMIT};
use pretty_assertions::assert_eq;

/// The longest identifier PostgreSQL keeps without truncating it.
const POSTGRESQL_IDENTIFIER_BYTE_LIMIT: usize = 63;

/// The two string columns of `current_attribute_values`, whose bound is the
/// B-tree placement rather than the identifier bound.
const PROJECTED_STRING_COLUMNS: [&str; 2] = ["value_string", "value_long_text"];

/// The two Relationship-object columns of `current_attribute_values`, whose
/// bound is the B-tree placement of [`INDEXED_URI_BYTE_LIMIT`].
const PROJECTED_URI_COLUMNS: [&str; 2] = ["value_uri", "value_long_uri"];

/// The constraints that place a Relationship object in exactly one of
/// [`PROJECTED_URI_COLUMNS`].
const PROJECTED_URI_CONSTRAINTS: [&str; 2] = [
    "chk_current_attribute_values_value_uri_length",
    "chk_current_attribute_values_value_long_uri_length",
];

/// Every constraint that holds an identifier column to [`IDENTIFIER_BYTE_LIMIT`].
const IDENTIFIER_LENGTH_CONSTRAINTS: [&str; 33] = [
    "chk_forwarded_subscription_routes_minted_id_length",
    "chk_attribute_names_name_length",
    "chk_entity_type_names_name_length",
    "chk_jsonld_contexts_id_length",
    "chk_jsonld_contexts_url_length",
    "chk_context_source_registrations_id_length",
    "chk_registration_delivery_state_registration_id_length",
    "chk_registration_information_registration_id_length",
    "chk_registration_information_entity_id_length",
    "chk_registration_information_entity_id_pattern_length",
    "chk_snapshots_id_length",
    "chk_snapshots_clone_source_id_length",
    "chk_entity_maps_id_length",
    "chk_entity_map_entries_entity_id_length",
    "chk_entity_map_entries_source_length",
    "chk_entity_map_links_registration_id_length",
    "chk_entity_map_links_linked_map_id_length",
    "chk_entities_id_length",
    "chk_temporal_entities_id_length",
    "chk_temporal_attribute_streams_dataset_id_length",
    "chk_attribute_history_verbatim_instance_id_length",
    "chk_attribute_history_instance_claims_id_length",
    "chk_scope_history_verbatim_instance_id_length",
    "chk_current_attribute_values_dataset_id_length",
    "chk_subscriptions_id_length",
    "chk_notification_events_entity_id_length",
    "chk_temporal_append_intents_entity_id_length",
    "chk_subscription_companions_local_subscription_id_length",
    "chk_subscription_companions_companion_subscription_id_length",
    "chk_forwarded_subscriptions_local_subscription_id_length",
    "chk_forwarded_subscriptions_registration_id_length",
    "chk_forwarded_subscriptions_remote_subscription_id_length",
    "chk_subscription_notified_registrations_registration_id_length",
];

/// One `octet_length(<column>) <op> <bound>` clause a migration states.
#[derive(Debug)]
struct OctetLengthBound {
    /// The column the clause measures.
    column: String,
    /// The comparison, `<=` or `>`.
    operator: String,
    /// The literal bound.
    bound: usize,
}

/// Every `octet_length(<column>) <op> <bound>` clause in `sql`.
fn octet_length_bounds(sql: &str) -> Vec<OctetLengthBound> {
    sql.match_indices("octet_length(")
        .filter_map(|(at, call)| {
            let rest = &sql[at + call.len()..];
            let (column, rest) = rest.split_once(')')?;
            let rest = rest.trim_start();
            let operator = ["<=", ">"].into_iter().find(|operator| rest.starts_with(operator))?;
            let digits: String = rest[operator.len()..].trim_start().chars().take_while(char::is_ascii_digit).collect();
            Some(OctetLengthBound {
                column: column.to_owned(),
                operator: operator.to_owned(),
                bound: digits.parse().ok()?,
            })
        })
        .collect()
}

/// Every name a migration declares with `CONSTRAINT <name>`.
fn declared_constraint_names(sql: &str) -> Vec<&str> {
    sql.split("CONSTRAINT ")
        .skip(1)
        .filter_map(|rest| rest.split(|character: char| character.is_whitespace()).next())
        .filter(|name| !name.is_empty())
        .collect()
}

#[test]
fn every_octet_length_bound_in_the_migrations_equals_its_rust_constant() {
    let bounds: Vec<OctetLengthBound> = every_migration_sql().into_iter().flat_map(octet_length_bounds).collect();
    assert!(bounds.len() >= IDENTIFIER_LENGTH_CONSTRAINTS.len(), "the scan found too few bounds: {bounds:?}");
    for bound in &bounds {
        let column = bound.column.as_str();
        let expected = if PROJECTED_STRING_COLUMNS.contains(&column) {
            INDEXED_STRING_BYTE_LIMIT
        } else if PROJECTED_URI_COLUMNS.contains(&column) {
            INDEXED_URI_BYTE_LIMIT
        } else {
            IDENTIFIER_BYTE_LIMIT
        };
        assert_eq!(bound.bound, expected, "{bound:?} disagrees with its Rust constant");
    }
    for overflow in ["value_long_text", "value_long_uri"] {
        let bound = bounds
            .iter()
            .find(|bound| bound.column == overflow)
            .unwrap_or_else(|| panic!("the overflow column {overflow} is bounded"));
        assert_eq!(bound.operator, ">", "{overflow} admits only what its indexed column refuses");
    }
    for indexed in ["value_string", "value_uri"] {
        let bound = bounds
            .iter()
            .find(|bound| bound.column == indexed)
            .unwrap_or_else(|| panic!("the indexed column {indexed} is bounded"));
        assert_eq!(bound.operator, "<=", "{indexed} admits only what its B-tree holds");
    }
}

#[test]
fn every_identifier_column_declares_its_length_constraint() {
    let declared: Vec<&str> = every_migration_sql().into_iter().flat_map(declared_constraint_names).collect();
    for constraint in IDENTIFIER_LENGTH_CONSTRAINTS.into_iter().chain(PROJECTED_URI_CONSTRAINTS) {
        assert!(declared.contains(&constraint), "no migration declares {constraint}");
    }
}

#[test]
fn every_declared_constraint_name_fits_a_postgresql_identifier() {
    for name in every_migration_sql().into_iter().flat_map(declared_constraint_names) {
        assert!(
            name.len() <= POSTGRESQL_IDENTIFIER_BYTE_LIMIT,
            "{name} is {} bytes, which PostgreSQL would truncate",
            name.len()
        );
    }
}

/// A Relationship traversal joins `entities.id` on `value_uri` alone, so every
/// object that can name a local Entity -- one no longer than an identifier --
/// must be placed in `value_uri` rather than in `value_long_uri`.
#[test]
fn every_object_that_can_name_a_local_entity_is_placed_in_the_traversed_column() {
    assert_eq!(INDEXED_URI_BYTE_LIMIT, IDENTIFIER_BYTE_LIMIT);
}
