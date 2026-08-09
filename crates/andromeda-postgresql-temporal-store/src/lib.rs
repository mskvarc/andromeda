//! PostgreSQL/TimescaleDB-backed temporal store for the NGSI-LD Temporal API.
//!
//! This crate owns the persistence layer for temporal entity attributes: the
//! `temporal_entity_attributes` catalogue, the `attribute_history` hypertable,
//! and the `scope_history` table.  It provides the [`TemporalRepository`]
//! (the concrete store), [`TemporalRecorder`] (the bulk-write coordinator),
//! and the backend-neutral port traits ([`TemporalHistoryReader`] and
//! [`TemporalMutator`]) that the service layer programs against.
//!
//! [`TemporalRepository`]: repo::TemporalRepository
//! [`TemporalRecorder`]: recorder::TemporalRecorder
//! [`TemporalHistoryReader`]: store::TemporalHistoryReader
//! [`TemporalMutator`]: store::TemporalMutator

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
mod entity_history;
/// One row of the temporal attribute catalogue.
pub mod entry;
/// Error types for the temporal store crate.
pub mod error;
/// Decoding `PgRow` into the backend-neutral temporal row types.
pub(crate) mod history_row_decode;
mod instance_mutation;
/// Row-to-domain conversions for temporal data.
pub mod mapper;
/// [`TemporalMutator`](store::TemporalMutator) implementation on [`TemporalRepository`](repo::TemporalRepository).
pub mod mutator;
/// Turning expanded entity payloads into the rows one temporal write commits.
pub mod observation;
/// Repository input parameter types for temporal data.
pub mod params;
/// Projection repair used by the asynchronous materializer and instance mutations.
pub mod projection_repair;
/// [`TemporalHistoryReader`](store::TemporalHistoryReader) implementation on [`TemporalRepository`](repo::TemporalRepository).
pub mod reader;
/// Writing attribute history whenever entities are created or modified.
pub mod recorder;
/// SQL-only repository for the NGSI-LD Temporal API.
pub mod repo;
/// Persistence-shaped row structs for temporal tables.
pub mod rows;
/// Backend-neutral temporal store types and trait declarations.
pub mod store;
/// Runtime capability probe for TimescaleDB TSL (Enterprise) features.
pub mod timescale_features;
/// The column an attribute's recorded values are stored in.
pub mod value_type;

#[cfg(test)]
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// Asserted here rather than by a central test so the failure names the
    /// crate that broke the rule, and so a crate added to the workspace cannot
    /// be forgotten by a table it was never added to.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// In-workspace crates this one may name, in any dependency table.
        /// Anything else is an upward or sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-layer-guard",
            "andromeda-ngsi-ld-geometry",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-model",
            "andromeda-ngsi-ld-query",
            "andromeda-ngsi-ld-vocabulary",
            "andromeda-postgresql-encoding",
            "andromeda-postgresql-query",
            "andromeda-postgresql-temporal-query",
        ];

        let forbidden = andromeda_layer_guard::manifest::baseline_forbidden_except(&["sqlx", "tokio"]);
        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), &forbidden, ALLOWED_IN_WORKSPACE);
    }
}
