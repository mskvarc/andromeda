//! Concrete Andromeda broker composition and process lifecycle.

#![deny(missing_docs)]

mod blocking_executor;
mod broker;
mod cached_context_invalidation;
mod component_report;
pub mod config;
mod context_hit_flush;
mod enforcement;
mod event;
mod linked_entity_forwarding;
mod linked_resolution_failure_mapping;
mod linked_traversal_parameters;
mod listeners;
mod logging;
pub mod outcome;
mod postgresql_maintenance;
mod registered_entity_probe;
mod retry_delay;
mod routes;
mod temporal_linked_forwarding;
mod tenant_factory;
mod tenant_state;
mod tenant_waker;

use crate::{config::runtime::RuntimeConfig, outcome::ProcessExitOutcome};

/// Run the configured broker and return its already-reported process outcome.
pub async fn run(config: RuntimeConfig) -> ProcessExitOutcome {
    broker::run(config).await
}
