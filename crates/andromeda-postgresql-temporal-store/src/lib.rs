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
pub(crate) mod aggregation_row_decode;
/// One row of the rebuilt `attribute_history` hypertable.
pub mod attribute_history_row;
/// The value one attribute-history row carries, and the column it occupies.
pub mod attribute_history_value;
/// Bulk inserts into the rebuilt `attribute_history` hypertable.
pub mod attribute_history_write;
/// Reading the parts of one expanded NGSI-LD attribute instance.
pub mod attribute_instance;
/// Translation between the NGSI-LD attribute family and its stored code.
pub mod attribute_kind_code;
/// The value an attribute instance carries, and the column it lands in.
pub mod attribute_value;
/// Dispatch mode for bulk history writes.
pub mod bulk_write_mode;
/// Binary COPY path for bulk attribute-history inserts.
pub(crate) mod copy;
/// The invariant that one client-supplied `instanceId` names one attribute
/// instance across every sibling `datasetId` stream of an attribute.
pub mod duplicate_instance_claim;
mod entity_attribute_history;
/// Error types for the temporal store crate.
pub mod error;
/// Decoding `PgRow` into the backend-neutral temporal row types.
pub(crate) mod history_row_decode;
mod instance_mutation;
/// Idempotent PostgreSQL implementation of deferred temporal appends.
pub mod intent_sink;
/// `andromeda-temporal` mutation-port implementation on [`TemporalRepository`](repo::TemporalRepository).
pub mod mutator;
/// `andromeda-temporal` history-reader implementation on [`TemporalRepository`](repo::TemporalRepository).
pub mod reader;
/// Writing attribute history whenever entities are created or modified.
pub mod recorder;
/// SQL-only repository for the NGSI-LD Temporal API.
pub mod repo;
/// Operator retention: ageing temporal storage out without NGSI-LD semantics.
pub mod retention_sweep;
/// One scope observation offered to the scope-history bulk insert.
pub mod scope_history_entry;
/// The value one scope-history row carries.
pub mod scope_history_value;
/// Bulk inserts into the rebuilt `scope_history` hypertable.
pub mod scope_history_write;
/// The NGSI-LD time axes a stored temporal row carries besides its write clock.
pub mod semantic_time_axes;
/// The JSON number a stored `NUMERIC` column names.
pub(crate) mod stored_decimal;
/// Splitting one stored instance identity across its two history columns.
pub mod stored_instance_columns;
/// Reading and pruning the temporal catalogue.
pub mod temporal_catalog_read;
/// Establishing the temporal catalogue rows one write's history hangs off.
pub mod temporal_catalog_write;
/// Temporal Evolution deletion and the orphan audit that checks it.
pub mod temporal_entity_deletion;
/// Running one compiled temporal statement against the tenant pool.
pub(crate) mod temporal_query_execution;
/// The grain one temporal attribute stream is keyed by.
pub mod temporal_stream;
/// Auditing stored unit-code identities against the unit-code catalog.
pub mod unit_code_audit;
/// Moving the NGSI-LD `unitCode` member between stored metadata and its column.
pub mod unit_code_metadata;
/// The write's own idempotency bookkeeping record.
pub(crate) mod write_record_persist;
/// When one write's idempotency record may be released.
pub(crate) mod write_record_release;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-commit",
        "andromeda-entity",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-postgresql-attribute-dictionary",
        "andromeda-postgresql-codes",
        "andromeda-postgresql-encoding",
        "andromeda-postgresql-query",
        "andromeda-postgresql-temporal-query",
        "andromeda-temporal",
        "andromeda-unit-catalog",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["sqlx", "tokio"],
}

#[cfg(test)]
mod port_contract_tests {
    use crate::{intent_sink::PostgresTemporalHistorySink, repo::TemporalRepository};
    use andromeda_temporal::port::{history_reader::TemporalHistoryReader, mutator::TemporalMutator, temporal_history_sink::TemporalHistorySink};

    fn assert_history_reader<T: TemporalHistoryReader>() {}

    fn assert_mutator<T: TemporalMutator>() {}

    fn assert_sink<T: TemporalHistorySink>() {}

    #[test]
    fn repository_implements_temporal_ports_directly() {
        assert_history_reader::<TemporalRepository>();
        assert_mutator::<TemporalRepository>();
        assert_sink::<PostgresTemporalHistorySink>();
    }
}
