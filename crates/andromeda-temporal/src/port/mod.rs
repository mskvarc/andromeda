//! Storage capabilities consumed by temporal policy.

pub mod history_reader;
pub mod mutation_barrier;
pub mod mutator;
/// Pending-intent reads used to preserve immediate logical visibility.
pub mod pending_intent_reader;
/// The caller-owned ports temporal policy is constructed over.
pub mod service_ports;
/// Idempotent temporal-history append capability.
pub mod temporal_history_sink;
/// Notification that a commit made deferred temporal work available.
pub mod temporal_intent_publisher;
/// Durable claim, acknowledgement and retry state for intent delivery.
pub mod temporal_intent_queue;
