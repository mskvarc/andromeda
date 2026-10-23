//! PostgreSQL/TimescaleDB-backed temporal store for the NGSI-LD Temporal API.
//!
//! This crate owns the persistence layer for Temporal Evolution: the
//! Evolution columns of the `entities` identity rows, the
//! `temporal_attribute_streams` catalogue and its stream type census, the
//! `attribute_history` hypertable and the `scope_history` table. It provides
//! the [`TemporalRepository`] concrete adapter
//! and the [`TemporalRecorder`] a commit records its history with, while
//! implementing the ports owned by `andromeda-temporal`.
//!
//! [`TemporalRepository`]: repo::TemporalRepository
//! [`TemporalRecorder`]: recorder::TemporalRecorder

#![deny(missing_docs)]

/// Decoding `PgRow` into the backend-neutral aggregation row and its group span.
pub(crate) mod aggregation_group_key_decode;
pub(crate) mod aggregation_row_decode;
/// Reading one entity's aggregation rows as the aggregates they compute.
pub(crate) mod aggregation_rows_read;
/// The value one attribute-history row carries, and the column it occupies.
pub mod attribute_history_value;
/// Deleting the Temporal Evolution of one attribute.
pub mod attribute_stream_deletion;
/// Running one storage-policy write without stalling the tenant behind it.
pub(crate) mod bounded_lock_wait;
/// Whether the attached `TimescaleDB` edition provides the hypercore columnstore.
pub mod columnstore_capability;
/// Converting aged history chunks to the columnstore, by the broker itself.
pub mod columnstore_conversion;
/// Enabling the hypercore columnstore on the temporal hypertables.
pub mod columnstore_enablement;
/// Letting a transaction decompress what its own confined statements must.
pub(crate) mod decompression_allowance;
/// The invariant that one client-supplied `instanceId` names one attribute
/// instance across every sibling `datasetId` stream of an attribute.
pub mod duplicate_instance_claim;
mod entity_attribute_history;
/// Error types for the temporal store crate.
pub mod error;
/// Giving up the Temporal Evolution of identity rows.
pub(crate) mod evolution_vacancy;
/// Reading which disqualifying shapes the instances a request aggregates carry.
pub(crate) mod held_shape_read;
/// Deciding which relations of a branch a temporal read takes history from.
mod history_branch;
/// What a history row stores beside its value, keyed as current state keys it.
pub mod history_members;
/// Removing the history of whole streams or of one entity's Scope.
pub(crate) mod history_removal;
/// Decoding `PgRow` into the backend-neutral temporal row types.
pub(crate) mod history_row_decode;
/// The identity and column reads the two history hypertables share.
pub(crate) mod history_row_identity;
/// The temporal hypertables whose storage policy is reconciled, and their columnstore layout.
pub mod hypertable_policy;
/// Reading the instances a split query's filters read in effect when its
/// window opens.
pub(crate) mod in_effect_instance_read;
mod instance_mutation;
/// The statements addressing one temporal attribute instance by its `instanceId`.
mod instance_statements;
/// Applying temporal append intents to a branch's history in SQL, for the
/// drain and the commit alike.
pub mod intent_application;
/// Aggregating one entity over its local and answered instances (clause 5.7.4.4).
pub(crate) mod merged_aggregation;
/// `andromeda-temporal` mutation-port implementation on [`TemporalRepository`](repo::TemporalRepository).
pub mod mutator;
/// Removing the families and value shapes a stream no longer holds.
/// Resolving the names and the attribute holders a temporal statement renders
/// against.
pub(crate) mod query_names_resolution;
/// `andromeda-temporal` history-reader implementation on [`TemporalRepository`](repo::TemporalRepository).
pub mod reader;
/// Writing attribute history whenever entities are created or modified.
pub mod recorder;
/// SQL-only repository for the NGSI-LD Temporal API.
pub mod repo;
/// Operator retention: ageing temporal storage out without NGSI-LD semantics.
pub mod retention_sweep;
pub(crate) mod scope_history_read;
/// The value one scope-history row carries.
pub mod scope_history_value;
/// The NGSI-LD time axes a stored temporal row carries besides its write clock.
pub mod semantic_time_axes;
/// Reading the leaves of a split temporal query's filters (clause 5.7.4.4).
pub(crate) mod split_filter_leaf_read;
/// Letting one transaction of work no request waits for run to its end.
pub(crate) mod statement_timeout_allowance;
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
pub(crate) mod stream_census;
pub(crate) mod stream_shape_growth;
pub(crate) mod stream_shape_pruning;
/// Reading the families and value shapes each attribute has been recorded with.
pub(crate) mod stream_shape_read;
/// Reading the temporal catalogue.
pub mod temporal_catalog_read;
/// Temporal Evolution deletion and the orphan audit that checks it.
pub mod temporal_entity_deletion;
/// One temporal append intent as the records the intent queue holds.
pub mod temporal_entry_record;
/// Running one compiled temporal statement against the tenant pool.
pub(crate) mod temporal_query_execution;
/// Conditions the temporal store's own statements and callers guarantee.
pub mod temporal_store_invariant;
/// Windowed temporal history deletion (the delete-by-query extension).
pub mod temporal_windowed_deletion;
/// Reading the instances inside a split query's window of the Relationships
/// its hops start from.
pub(crate) mod window_relationship_read;

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
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-postgresql-pool",
    ],
    allow_production_dependencies: ["sea-query", "sqlx", "tokio"],
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
