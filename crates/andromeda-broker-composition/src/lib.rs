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
mod extension_routing;
mod linked_forwarding;
mod listener_tls;
mod listeners;
mod local_entity_type;
mod logging;
pub mod outcome;
mod planes;
mod postgresql_maintenance;
mod registered_entity_probe;
mod routes;
mod shutdown_signal;
mod startup_report;
mod tenant_factory;
mod tenant_state;
mod tenant_waker;

use crate::outcome::ProcessExitOutcome;
use andromeda_configuration::broker_config::BrokerConfig;

/// Run the configured broker and return its already-reported process outcome.
pub async fn run(config: BrokerConfig) -> ProcessExitOutcome {
    broker::run(config).await
}
