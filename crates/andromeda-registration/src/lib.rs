//! Context-source registration policy (ETSI GS CIM 009 clause 5.9).

#![deny(missing_docs)]

use crate::{entity_probe::RegisteredEntityProbe, store::RegistrationStore};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_model::notification::CsrChangeEvent;
use std::sync::Arc;

mod context_source_properties;
mod create;
mod delete;
/// The entity-store questions proxied-registration admission asks.
pub mod entity_probe;
/// Registration policy failures.
pub mod error;
pub mod expiry;
#[cfg(test)]
mod fixed_probe;
#[cfg(test)]
mod fixed_store;
mod geometry;
mod identifier;
mod information_expansion;
mod mode_rules;
pub mod nullable_member;
mod payload;
mod query;
/// Fully resolved registration update values and merge policy.
pub mod resolved_update;
mod retrieve;
/// The registration store port, and the parameter shapes it takes.
pub mod store;
mod update;
mod validation;

/// Service for managing NGSI-LD context source registrations.
pub struct RegistrationService {
    repo: Arc<dyn RegistrationStore>,
    entity_probe: Arc<dyn RegisteredEntityProbe>,
    engine: JsonLdEngine,
    change_sink: Option<Arc<dyn RegistrationChangeSink>>,
}

/// Publishes registration lifecycle changes without coupling policy to a runtime transport.
pub trait RegistrationChangeSink: Send + Sync + 'static {
    /// Publish one completed registration change.
    fn publish(&self, event: CsrChangeEvent);
}

impl RegistrationService {
    /// Create a context-source registration service backed by the supplied
    /// store, the local-entity probe proxied-registration admission consults,
    /// and JSON-LD processor.
    pub const fn new(
        repo: Arc<dyn RegistrationStore>,
        entity_probe: Arc<dyn RegisteredEntityProbe>,
        engine: JsonLdEngine,
        change_sink: Option<Arc<dyn RegistrationChangeSink>>,
    ) -> RegistrationService {
        RegistrationService {
            repo,
            entity_probe,
            engine,
            change_sink,
        }
    }

    fn emit_csr_change(&self, event: CsrChangeEvent) {
        if let Some(ref sink) = self.change_sink {
            sink.publish(event);
        }
    }
}

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-jsonld",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-request",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}
