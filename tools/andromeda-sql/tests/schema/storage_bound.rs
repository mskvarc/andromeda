//! The byte bounds the migrations state, tied to the Rust constants the broker
//! enforces them with.
//!
//! A bound stated twice (once as a check a migration declares and once as the
//! constant a parser refuses input with) drifts the moment one side changes
//! alone. These tests read the committed migrations, so a migration whose
//! literal disagrees with the constant fails the build rather than surfacing as
//! a constraint violation on a request the parser admitted.

use crate::repository::{PinError, constraint, schema};
use andromeda_ngsi_ld_lexicon::{identifier_length::IDENTIFIER_BYTE_LIMIT, tenant::TENANT_NAME_BYTE_LIMIT};
use andromeda_ngsi_ld_request::pagination::query_digest::QUERY_DIGEST_BYTES;
use andromeda_postgresql_schema::{
    string_value_column::INDEXED_STRING_BYTE_LIMIT,
    tenancy::{DATABASE_NAME_BYTE_LIMIT, EXTENSION_NAME_BYTE_LIMIT, EXTENSION_VERSION_BYTE_LIMIT},
    uri_value_column::INDEXED_URI_BYTE_LIMIT,
};
use andromeda_sql::declared_schema::{
    DeclaredSchema,
    ResolvedType,
    check_bound::{Bound, ColumnBound, Comparison, bounds_of},
    column::DeclaredColumn,
    constraint::ConstraintKind,
    declared_type::BuiltinType,
    parsed_migration::ParsedMigration,
};
use pretty_assertions::assert_eq;

/// The two string columns of `current_attributes`, whose bound is the B-tree
/// placement rather than the identifier bound.
const PROJECTED_STRING_COLUMNS: [&str; 2] = ["value_string", "value_long_text"];

/// The two Relationship-object columns of `current_attributes`, whose bound
/// is the B-tree placement of [`INDEXED_URI_BYTE_LIMIT`].
const PROJECTED_URI_COLUMNS: [&str; 2] = ["value_uri", "value_long_uri"];

/// The constraints that place a Relationship object in exactly one of
/// [`PROJECTED_URI_COLUMNS`].
const PROJECTED_URI_CONSTRAINTS: [&str; 2] = ["chk_ca_value_uri_length", "chk_ca_value_long_uri_length"];

/// The version columns of `tenant_extension_versions`, held to
/// [`EXTENSION_VERSION_BYTE_LIMIT`].
const EXTENSION_VERSION_COLUMNS: [&str; 2] = ["installed_version", "shipped_version"];

/// Every constraint that holds an identifier column to [`IDENTIFIER_BYTE_LIMIT`].
const IDENTIFIER_LENGTH_CONSTRAINTS: [&str; 28] = [
    "chk_forwarded_subscription_routes_minted_subscription_id_length",
    "chk_attribute_names_name_length",
    "chk_entity_type_names_name_length",
    "chk_jsonld_contexts_id_length",
    "chk_jsonld_contexts_url_length",
    "chk_csr_id_length",
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
    "chk_temporal_attribute_streams_dataset_key_length",
    "chk_attribute_history_verbatim_instance_id_length",
    "chk_attribute_history_instance_claims_id_length",
    "chk_ca_dataset_key_length",
    "chk_subscriptions_id_length",
    "chk_notification_events_entity_id_length",
    "chk_temporal_append_intents_entity_id_length",
    "chk_forwarded_subscriptions_registration_id_length",
    "chk_forwarded_subscriptions_remote_subscription_id_length",
    "chk_subscription_notified_registrations_registration_id_length",
];

/// The `text` columns of `current_attributes` no check bounds, each with the
/// reason its migration comment gives.
const UNBOUNDED_INSTANCE_TEXT_COLUMNS: [(&str, &str); 1] = [("value_language", "no index keys it")];

/// Every byte-length bound a check states, with the table it is on.
fn byte_length_bounds(schema: &DeclaredSchema) -> Vec<(&str, ColumnBound)> {
    schema
        .constraints()
        .iter()
        .filter(|constraint| constraint.kind() == ConstraintKind::Check)
        .filter_map(|constraint| Some((constraint.table(), constraint.expression()?)))
        .flat_map(|(table, expression)| bounds_of(expression).into_iter().map(move |bound| (table, bound)))
        .filter(|(_, bound)| matches!(bound.bound(), Bound::ByteLength(..)))
        .collect()
}

#[test]
fn every_octet_length_bound_in_the_migrations_equals_its_rust_constant() -> Result<(), PinError> {
    let schema = schema()?;
    let bounds = byte_length_bounds(&schema);
    assert!(bounds.len() >= IDENTIFIER_LENGTH_CONSTRAINTS.len(), "the scan found too few bounds: {bounds:?}");
    for (table, bound) in &bounds {
        let column = bound.column();
        let expected = if (*table, column) == ("tenants", "name") {
            TENANT_NAME_BYTE_LIMIT
        } else if (*table, column) == ("tenants", "database_name") {
            DATABASE_NAME_BYTE_LIMIT
        } else if (*table, column) == ("tenant_extension_versions", "extension") {
            EXTENSION_NAME_BYTE_LIMIT
        } else if *table == "tenant_extension_versions" && EXTENSION_VERSION_COLUMNS.contains(&column) {
            EXTENSION_VERSION_BYTE_LIMIT
        } else if (*table, column) == ("posted_queries", "digest") {
            QUERY_DIGEST_BYTES
        } else if PROJECTED_STRING_COLUMNS.contains(&column) {
            INDEXED_STRING_BYTE_LIMIT
        } else if PROJECTED_URI_COLUMNS.contains(&column) {
            INDEXED_URI_BYTE_LIMIT
        } else {
            IDENTIFIER_BYTE_LIMIT
        };
        let Bound::ByteLength(_, value) = bound.bound() else {
            continue;
        };
        assert_eq!(
            usize::try_from(value).ok(),
            Some(expected),
            "{table}.{column}: {bound:?} disagrees with its Rust constant"
        );
    }
    let comparison_of = |column: &str| bounds.iter().find(|(_, bound)| bound.column() == column).map(|(_, bound)| bound.bound());
    for overflow in ["value_long_text", "value_long_uri"] {
        assert!(
            matches!(comparison_of(overflow), Some(Bound::ByteLength(Comparison::Greater, _))),
            "{overflow} admits only what its indexed column refuses"
        );
    }
    for indexed in ["value_string", "value_uri"] {
        assert!(
            matches!(comparison_of(indexed), Some(Bound::ByteLength(Comparison::LessOrEqual, _))),
            "{indexed} admits only what its B-tree holds"
        );
    }
    Ok(())
}

#[test]
fn every_identifier_column_declares_its_length_constraint() -> Result<(), PinError> {
    let schema = schema()?;
    for name in IDENTIFIER_LENGTH_CONSTRAINTS.into_iter().chain(PROJECTED_URI_CONSTRAINTS) {
        assert_eq!(constraint(&schema, name)?.kind(), ConstraintKind::Check, "{name}");
    }
    Ok(())
}

/// A Relationship traversal joins `entities.id` on `value_uri` alone, so every
/// object that can name a local Entity (one no longer than an identifier) must
/// be placed in `value_uri` rather than in `value_long_uri`.
#[test]
fn every_object_that_can_name_a_local_entity_is_placed_in_the_traversed_column() {
    assert_eq!(INDEXED_URI_BYTE_LIMIT, IDENTIFIER_BYTE_LIMIT);
}

#[test]
fn every_current_attribute_text_column_is_bounded_or_documented_unbounded() -> Result<(), PinError> {
    let schema = schema()?;
    let bounded: Vec<String> = byte_length_bounds(&schema)
        .into_iter()
        .filter(|(table, _)| *table == "current_attributes")
        .map(|(_, bound)| bound.column().to_owned())
        .collect();
    let text_columns: Vec<&str> = schema
        .columns()
        .iter()
        .filter(|column| {
            column.relation() == "current_attributes"
                && schema.resolve(column.data_type())
                    == ResolvedType::Builtin {
                        element: BuiltinType::Text,
                        array: false,
                    }
        })
        .map(DeclaredColumn::name)
        .collect();
    assert_eq!(text_columns.len(), 6, "{text_columns:?}");
    let migration: String = schema
        .migrations()
        .iter()
        .map(ParsedMigration::text)
        .filter(|text| text.contains("create table current_attributes"))
        .collect();
    for column in text_columns {
        match UNBOUNDED_INSTANCE_TEXT_COLUMNS.iter().find(|(unbounded, _)| *unbounded == column) {
            Some((_, reason)) => {
                assert!(!bounded.iter().any(|bound| bound == column), "{column} is bounded after all");
                assert!(migration.contains(reason), "the migration does not say of {column} that {reason}");
            }
            None => assert!(bounded.iter().any(|bound| bound == column), "{column} has no length check"),
        }
    }
    Ok(())
}

/// A composite type takes no check, so the element's strings and URIs are
/// unbounded, which its migration comment justifies.
#[test]
fn the_sub_attribute_element_documents_why_its_text_fields_are_unbounded() -> Result<(), PinError> {
    let schema = schema()?;
    let prose: String = schema
        .migrations()
        .iter()
        .map(ParsedMigration::text)
        .filter(|text| text.contains("create type sub_attribute"))
        .flat_map(str::lines)
        .filter_map(|line| line.trim_start().strip_prefix("-- "))
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        prose.contains("fields carry no length bound and no overflow field: no B-tree keys an element's value"),
        "{prose}"
    );
    Ok(())
}
