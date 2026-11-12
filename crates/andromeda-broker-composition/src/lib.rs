//! Concrete Andromeda broker composition and process lifecycle.

mod background_task_health;
mod blocking_executor;
mod broker;
mod broker_health;
pub mod build_metadata;
mod cached_context_invalidation;
mod component_report;
pub mod configuration;
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
mod pending_work_probe;
mod planes;
mod postgresql_maintenance;
mod readiness_probe;
mod registered_entity_probe;
mod routes;
mod shutdown_signal;
mod startup_report;
mod stop_deadline;
mod tenancy_startup;
mod tenant_factory;
mod tenant_janitor;
mod tenant_loading;
mod tenant_state;
mod tenant_waker;

use crate::outcome::ProcessExitOutcome;
use andromeda_configuration::{broker_config::BrokerConfig, load::LoadedConfiguration};

/// Run the configured broker and return its already-reported process outcome.
pub async fn run(loaded: LoadedConfiguration) -> ProcessExitOutcome {
    broker::run(loaded).await
}

/// Probe the readiness of the broker `config` describes and return the
/// already-reported process outcome.
pub async fn probe_readiness(config: &BrokerConfig) -> ProcessExitOutcome {
    readiness_probe::probe_reported(&config.server, &config.management).await
}
