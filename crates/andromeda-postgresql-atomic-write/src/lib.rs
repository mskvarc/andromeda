//! PostgreSQL implementation of the atomic context-commit protocol.
//!
//! The crate owns the transaction that commits current entity state, native
//! PostgreSQL temporal history, the `current_attribute_values` projection,
//! notification events and durable asynchronous side-effect jobs. All
//! of them share one transaction, so a failure in any of them rolls the entity
//! mutation back. PostgreSQL statement fusion remains a private implementation
//! detail.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

mod columns;
mod commit_refusal;
/// Native PostgreSQL implementation of the context commit port.
pub mod committer;
/// PostgreSQL atomic-write execution configuration.
pub mod config;
mod consequences;
mod deferred_history;
mod entity_group;
mod history;
/// Native or deferred temporal history selection.
pub mod history_mode;
/// PostgreSQL durable temporal intent queue.
pub mod intent_store;
mod projection;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-commit",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-postgresql-codes",
        "andromeda-postgresql-context-store",
        "andromeda-postgresql-query",
        "andromeda-postgresql-temporal-store",
        "andromeda-temporal",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-model",
    ],
    allow_production_dependencies: ["confique", "sqlx"],
}

#[cfg(test)]
mod port_contract_tests {
    use crate::intent_store::PostgresTemporalIntentStore;
    use andromeda_temporal::port::{pending_intent_reader::PendingTemporalIntentReader, temporal_intent_queue::TemporalIntentQueue};

    fn assert_pending_reader<T: PendingTemporalIntentReader>() {}

    fn assert_intent_queue<T: TemporalIntentQueue>() {}

    #[test]
    fn postgres_anchor_implements_temporal_intent_ports_directly() {
        assert_pending_reader::<PostgresTemporalIntentStore>();
        assert_intent_queue::<PostgresTemporalIntentStore>();
    }
}
