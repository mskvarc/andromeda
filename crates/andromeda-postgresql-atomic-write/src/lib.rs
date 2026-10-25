//! PostgreSQL implementation of the atomic context-commit protocol.
//!
//! The crate owns the transaction that commits current entity state, native
//! PostgreSQL temporal history, notification events and durable asynchronous
//! side-effect jobs. All
//! of them share one transaction, so a failure in any of them rolls the entity
//! mutation back. PostgreSQL statement fusion remains a private implementation
//! detail.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

mod attempt_failure;
/// Recording a live synchronous commit's history behind its Entities' queued
/// history.
mod behind_queue;
mod columns;
mod commit_refusal;
/// Native PostgreSQL implementation of the context commit port.
pub mod committer;
mod consequences;
mod contention_retry;
mod deferred_history;
mod entity_group;
mod history;
mod history_interpretation;
/// Whether a commit records its temporal history inline or queues it.
pub mod history_mode;
/// PostgreSQL durable temporal intent queue.
pub mod intent_store;
/// Removing the side-effect job a replayed commit settles.
mod job_settlement;
mod name_resolution;
mod write_isolation;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-commit",
        "andromeda-context-space",
        "andromeda-ngsi-ld-entity",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-persistence-failure",
        "andromeda-postgresql-attribute-instance",
        "andromeda-postgresql-branch",
        "andromeda-postgresql-codes",
        "andromeda-postgresql-context-store",
        "andromeda-postgresql-name-dictionary",
        "andromeda-postgresql-pool",
        "andromeda-postgresql-query",
        "andromeda-postgresql-schema",
        "andromeda-postgresql-seaquery",
        "andromeda-postgresql-temporal-store",
        "andromeda-retry",
        "andromeda-telemetry",
        "andromeda-temporal",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-ngsi-ld-number",
    ],
    // `tokio` for the backoff between runs of a commit PostgreSQL rolled back
    // as a deadlock or serialization victim (`contention_retry`).
    allow_production_dependencies: ["sea-query", "sqlx", "tokio"],
}

#[cfg(test)]
mod port_contract_tests {
    use crate::intent_store::PostgresTemporalIntentStore;
    use andromeda_temporal::port::{temporal_intent_application::TemporalIntentApplication, temporal_intent_queue::TemporalIntentQueue};

    fn assert_application<T: TemporalIntentApplication>() {}

    fn assert_intent_queue<T: TemporalIntentQueue>() {}

    #[test]
    fn postgres_anchor_implements_temporal_intent_ports_directly() {
        assert_application::<PostgresTemporalIntentStore>();
        assert_intent_queue::<PostgresTemporalIntentStore>();
    }
}
