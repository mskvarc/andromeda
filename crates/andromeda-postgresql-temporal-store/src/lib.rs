//! PostgreSQL/TimescaleDB-backed temporal store for the NGSI-LD Temporal API.
//!
//! This crate owns the persistence layer for Temporal Evolution: the
//! `temporal_entities` and `temporal_attribute_streams` catalogue, the
//! `attribute_history` and `scope_history` hypertables, and the `write_records`
//! idempotency relation. It provides the [`TemporalRepository`] concrete adapter
//! and [`TemporalRecorder`] bulk-write coordinator while implementing the ports
//! owned by `andromeda-temporal`.
//!
//! [`TemporalRepository`]: repo::TemporalRepository
//! [`TemporalRecorder`]: recorder::TemporalRecorder

#![deny(missing_docs)]

/// Decoding `PgRow` into the backend-neutral aggregation row and its group span.
pub(crate) mod aggregation_group_key_decode;
pub(crate) mod aggregation_row_decode;
/// Reading one entity's aggregation rows as the aggregates they compute.
pub(crate) mod aggregation_rows_read;
/// The column-major binds one attribute-history batch travels in.
pub(crate) mod attribute_history_binds;
/// One row of the `attribute_history` hypertable.
pub mod attribute_history_row;
/// The value one attribute-history row carries, and the column it occupies.
pub mod attribute_history_value;
/// Bulk inserts into the `attribute_history` hypertable.
pub mod attribute_history_write;
/// Deleting the Temporal Evolution of one attribute.
pub mod attribute_stream_deletion;
/// Whether the attached `TimescaleDB` edition provides the hypercore columnstore.
pub mod columnstore_capability;
/// Enabling the hypercore columnstore on the temporal hypertables.
pub mod columnstore_enablement;
/// The invariant that one client-supplied `instanceId` names one attribute
/// instance across every sibling `datasetId` stream of an attribute.
pub mod duplicate_instance_claim;
mod entity_attribute_history;
/// Error types for the temporal store crate.
pub mod error;
/// What a history row stores beside its value, keyed as current state keys it.
pub mod history_members;
/// Decoding `PgRow` into the backend-neutral temporal row types.
pub(crate) mod history_row_decode;
/// The identity and column reads the two history hypertables share.
pub(crate) mod history_row_identity;
mod instance_mutation;
/// The statements addressing one temporal attribute instance by its `instanceId`.
mod instance_statements;
/// Applying queued temporal append intents to history in SQL.
pub mod intent_application;
/// Recording a commit's temporal append intents as history rows.
pub(crate) mod intent_sink;
/// Aggregating one entity over its local and answered instances (clause 5.7.4.4).
pub(crate) mod merged_aggregation;
/// `andromeda-temporal` mutation-port implementation on [`TemporalRepository`](repo::TemporalRepository).
pub mod mutator;
/// `andromeda-temporal` history-reader implementation on [`TemporalRepository`](repo::TemporalRepository).
pub mod reader;
/// Writing attribute history whenever entities are created or modified.
pub mod recorder;
/// SQL-only repository for the NGSI-LD Temporal API.
pub mod repo;
/// The Temporal Evolutions a chunk resolved, and which of them it created.
pub(crate) mod resolved_evolutions;
/// Operator retention: ageing temporal storage out without NGSI-LD semantics.
pub mod retention_sweep;
/// TimescaleDB procedural DDL SeaQuery has no form for.
pub mod schema_ddl;
/// One scope observation offered to the scope-history bulk insert.
pub mod scope_history_entry;
pub(crate) mod scope_history_read;
/// The value one scope-history row carries.
pub mod scope_history_value;
/// Bulk inserts into the `scope_history` hypertable.
pub mod scope_history_write;
/// The NGSI-LD time axes a stored temporal row carries besides its write clock.
pub mod semantic_time_axes;
/// Reading the leaves of a split temporal query's filters (clause 5.7.4.4).
pub(crate) mod split_filter_leaf_read;
/// Reconciling the temporal hypertables' storage policy with configuration.
pub mod storage_policy;
/// What an aggregation row that does not read back looks like.
pub mod stored_aggregation_error;
/// The family and value shape one attribute-history row was written with.
pub(crate) mod stored_attribute_shape;
/// What a temporal catalogue row that no longer reads back looks like.
pub mod stored_catalog_error;
/// What a history row that no longer reads back looks like.
pub mod stored_history_error;
/// Splitting one stored instance identity across its two history columns.
pub mod stored_instance_columns;
/// The stream grains one chunk requests, bound column-major and indexed back.
pub(crate) mod stream_grain_binds;
/// Reading the temporal catalogue.
pub mod temporal_catalog_read;
/// Coordinating one chunk's temporal-entity and temporal-stream catalogue writes.
pub mod temporal_catalog_write;
/// Establishing the `temporal_entities` half of the catalogue for a chunk.
pub mod temporal_entity_catalog_write;
/// Temporal Evolution deletion and the orphan audit that checks it.
pub mod temporal_entity_deletion;
/// One temporal append intent as the records the intent queue holds.
pub mod temporal_entry_record;
/// Running one compiled temporal statement against the tenant pool.
pub(crate) mod temporal_query_execution;
/// Conditions the temporal store's own statements and callers guarantee.
pub mod temporal_store_invariant;
/// The grain one temporal attribute stream is keyed by.
pub mod temporal_stream;
/// Establishing the `temporal_attribute_streams` half of the catalogue for a chunk.
pub mod temporal_stream_catalog_write;
/// Windowed temporal history deletion (the delete-by-query extension).
pub mod temporal_windowed_deletion;
/// The clause 4.5.7 verbatim-`instanceId` uniqueness pass a history batch runs.
pub(crate) mod verbatim_instance_claim;
/// The write's own idempotency bookkeeping record.
pub(crate) mod write_record_persist;
/// When one write's idempotency record may be released.
pub(crate) mod write_record_release;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-space",
        "andromeda-context-commit",
        "andromeda-entity",
        "andromeda-ngsi-ld-entity",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-persistence-failure",
        "andromeda-postgresql-attribute-instance",
        "andromeda-postgresql-branch",
        "andromeda-postgresql-name-dictionary",
        "andromeda-postgresql-pool",
        "andromeda-postgresql-codes",
        "andromeda-postgresql-collation",
        "andromeda-postgresql-encoding",
        "andromeda-postgresql-query",
        "andromeda-postgresql-schema",
        "andromeda-postgresql-seaquery",
        "andromeda-postgresql-temporal-query",
        "andromeda-telemetry",
        "andromeda-temporal",
        "andromeda-unit-catalog",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["sea-query", "sea-query-sqlx", "sqlx", "tokio"],
}

#[cfg(test)]
mod port_contract_tests {
    use crate::repo::TemporalRepository;
    use andromeda_temporal::port::{history_reader::TemporalHistoryReader, mutator::TemporalMutator};

    fn assert_history_reader<T: TemporalHistoryReader>() {}

    fn assert_mutator<T: TemporalMutator>() {}

    #[test]
    fn repository_implements_temporal_ports_directly() {
        assert_history_reader::<TemporalRepository>();
        assert_mutator::<TemporalRepository>();
    }
}
