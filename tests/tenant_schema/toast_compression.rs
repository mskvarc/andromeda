//! TOAST compression: every wide write-once column declares lz4.
//!
//! Every variable-length column that is written once and read many times -
//! the attribute values, the temporal values, the JSONB configuration
//! documents of subscriptions, registrations, snapshots and contexts, and the
//! identifiers that ride beside them - is declared `COMPRESSION lz4`, because
//! lz4 decompresses far faster than the pglz default for the same width and the
//! read side is where the broker spends its time. The declaration is per column
//! and silently absent when a migration forgets it, so the catalogue is read
//! back: `pg_attribute.attcompression` is `l` for lz4, `p` for pglz and empty
//! for a column that inherits `default_toast_compression`.

use crate::SUITE;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use sqlx::PgPool;

/// Every `(relation, column)` the schema declares with lz4 TOAST compression.
const LZ4_COLUMNS: &[(&str, &str)] = &[
    ("entities", "id"),
    ("entity_search", "scope_paths"),
    ("jsonld_contexts", "id"),
    ("jsonld_contexts", "url"),
    ("jsonld_contexts", "document_url"),
    ("jsonld_contexts", "payload"),
    ("jsonld_contexts", "extra_info"),
    ("temporal_entities", "id"),
    ("temporal_attribute_streams", "dataset_id"),
    ("attribute_history", "value_string"),
    ("attribute_history", "value_object"),
    ("attribute_history", "object_type_keys"),
    ("attribute_history", "proof_seals"),
    ("attribute_history", "sub_attributes"),
    ("current_attributes", "dataset_id"),
    ("current_attributes", "value_string"),
    ("current_attributes", "value_object"),
    ("current_attributes", "value_long_text"),
    ("current_attributes", "value_long_uri"),
    ("current_attributes", "value_geometry_members"),
    ("current_attributes", "sub_attributes"),
    ("subscriptions", "entities"),
    ("subscriptions", "notification"),
    ("subscriptions", "geo_q"),
    ("subscriptions", "temporal_q"),
    ("context_source_registrations", "id"),
    ("context_source_registrations", "information"),
    ("context_source_registrations", "additional_properties"),
    ("context_source_registrations", "management"),
    ("registration_information", "registration_id"),
    ("registration_information", "entity_types"),
    ("snapshots", "id"),
    ("snapshots", "snapshot_queries"),
    ("snapshots", "snapshot_temporal_queries"),
    ("notification_events", "entity_id"),
    ("notification_events", "entity_snapshot"),
    ("pending_side_effects", "payload"),
    ("pending_side_effects", "last_error"),
    ("temporal_append_intents", "entity_id"),
    ("temporal_append_intents", "entity_type_name_keys"),
    ("temporal_append_intents", "entries"),
    ("temporal_append_intents", "last_error"),
];

/// Read the compression method one column declares: `l` for lz4, `p` for
/// pglz, and the empty string when the column inherits the server default.
async fn column_compression(pool: &PgPool, relation: &str, column: &str) -> String {
    sqlx::query_scalar(
        "SELECT a.attcompression::text FROM pg_attribute AS a \
         JOIN pg_class AS c ON c.oid = a.attrelid \
         JOIN pg_namespace AS n ON n.oid = c.relnamespace \
         WHERE n.nspname = current_schema() AND c.relname = $1 AND a.attname = $2 AND a.attnum > 0 AND NOT a.attisdropped",
    )
    .bind(relation)
    .bind(column)
    .fetch_one(pool)
    .await
    .expect("the inspected column must exist")
}

#[tokio::test]
async fn every_write_once_read_many_varlena_column_declares_lz4_compression() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();

    for (relation, column) in LZ4_COLUMNS {
        assert_eq!(
            column_compression(pool, relation, column).await,
            "l",
            "{relation}.{column} must be stored with lz4 TOAST compression"
        );
    }

    scope.finish().await.expect("cleanup failed");
}
