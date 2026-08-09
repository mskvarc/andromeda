//! PostgreSQL/TimescaleDB-backed temporal store for the NGSI-LD Temporal API.
//!
//! This crate owns the persistence layer for temporal entity attributes: the
//! `temporal_entity_attributes` catalogue, the `attribute_history` hypertable,
//! and the `scope_history` table. It provides the [`TemporalRepository`] concrete adapter and [`TemporalRecorder`] bulk-write coordinator while implementing the ports owned by `andromeda-temporal`.
//!
//! [`TemporalRepository`]: repo::TemporalRepository
//! [`TemporalRecorder`]: recorder::TemporalRecorder

#![deny(missing_docs)]

mod attribute_history;
/// Reading the parts of one expanded NGSI-LD attribute instance.
pub mod attribute_instance;
/// The value an attribute instance carries, and the column it lands in.
pub mod attribute_value;
mod batch_write;
/// Dispatch mode for bulk history writes.
pub mod bulk_write_mode;
/// Binary COPY path for bulk attribute-history inserts.
pub(crate) mod copy;
mod entity_attribute_history;
mod entity_history;
/// One row of the temporal attribute catalogue.
pub mod entry;
/// Error types for the temporal store crate.
pub mod error;
/// Decoding `PgRow` into the backend-neutral temporal row types.
pub(crate) mod history_row_decode;
mod instance_mutation;
/// Idempotent PostgreSQL implementation of deferred temporal appends.
pub mod intent_sink;
/// Row-to-domain conversions for temporal data.
pub mod mapper;
/// `andromeda-temporal` mutation-port implementation on [`TemporalRepository`](repo::TemporalRepository).
pub mod mutator;
/// Repository input parameter types for temporal data.
pub mod params;
/// Projection repair used by the asynchronous materializer and instance mutations.
pub mod projection_repair;
/// `andromeda-temporal` history-reader implementation on [`TemporalRepository`](repo::TemporalRepository).
pub mod reader;
/// Writing attribute history whenever entities are created or modified.
pub mod recorder;
/// SQL-only repository for the NGSI-LD Temporal API.
pub mod repo;
/// Persistence-shaped row structs for temporal tables.
pub mod rows;
/// Runtime capability probe for TimescaleDB TSL (Enterprise) features.
pub mod timescale_features;
/// The column an attribute's recorded values are stored in.
pub mod value_type;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-entity",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-postgresql-encoding",
        "andromeda-postgresql-context-store",
        "andromeda-postgresql-query",
        "andromeda-postgresql-temporal-query",
        "andromeda-temporal",
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
