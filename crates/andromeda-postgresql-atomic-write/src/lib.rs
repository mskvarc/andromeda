//! PostgreSQL implementation of the atomic context-commit protocol.
//!
//! The crate owns the transaction that commits current entity state, native
//! PostgreSQL temporal history, notification outbox entries and durable
//! asynchronous side-effect jobs. PostgreSQL statement fusion and projection
//! publication remain private implementation details.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

mod columns;
/// Native PostgreSQL implementation of the context commit port.
pub mod committer;
mod deferred_history;
mod entity_group;
mod history;
/// Native or deferred temporal history selection.
pub mod history_mode;
/// PostgreSQL durable temporal intent queue.
pub mod intent_store;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-commit",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-postgresql-context-store",
        "andromeda-postgresql-query",
        "andromeda-postgresql-temporal-store",
        "andromeda-temporal",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-ngsi-ld-geometry",
    ],
    allow_production_dependencies: ["sqlx"],
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
