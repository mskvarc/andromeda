//! Concrete Andromeda broker composition and process lifecycle.

#![deny(missing_docs)]

mod blocking_executor;
mod broker;
mod component_report;
pub mod config;
mod context_hit_flush;
mod enforcement;
mod event;
pub mod outcome;
mod routes;
mod tenant_factory;
mod tenant_state;

use crate::{config::runtime::RuntimeConfig, outcome::ProcessExitOutcome};

/// Run the configured broker and return its already-reported process outcome.
pub async fn run(config: RuntimeConfig) -> ProcessExitOutcome {
    broker::run(config).await
}
