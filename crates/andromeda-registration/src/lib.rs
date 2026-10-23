//! Context-source registration policy (ETSI GS CIM 009 clause 5.9).

#![deny(missing_docs)]

use crate::{entity_probe::RegisteredEntityProbe, removal_hook::RegistrationRemovalHook, store::RegistrationStore};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_model::notification::CsrChangeEvent;
use andromeda_ngsi_ld_query::geo::admission::GeoQueryAdmission;
use std::sync::Arc;

mod context_source_properties;
mod create;
/// The members of a `ContextSourceRegistration` payload.
pub mod csr_member;
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
mod forwarding_context;
mod geometry;
mod identifier;
pub mod information_compaction;
mod information_expansion;
mod mode_rules;
pub mod nullable_member;
pub mod query;
/// The Create and Update Context Source Registration request body.
pub mod registration_body;
mod registration_body_reading;
mod registration_compaction;
/// The typed reasons a registration collides with existing context data.
pub mod registration_conflict_reason;
/// The invariants registration policy relies on.
pub mod registration_invariant;
/// The typed reasons a registration request is rejected as `BadRequestData`.
pub mod registration_validation_reason;
/// The distributed side of removing a registration, as a port.
pub mod removal_hook;
/// Fully resolved registration update values and merge policy.
pub mod resolved_update;
mod retrieve;
#[cfg(test)]
mod service_fixture;
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
    removal_hook: Option<Arc<dyn RegistrationRemovalHook>>,
    /// What a discovery geo-query is admitted under; its limits also bound
    /// a geometry the registration writes.
    geo_admission: GeoQueryAdmission,
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
        geo_admission: GeoQueryAdmission,
    ) -> RegistrationService {
        RegistrationService {
            repo,
            entity_probe,
            engine,
            change_sink,
            removal_hook: None,
            geo_admission,
        }
    }

    /// Attach the distributed side of registration removal.
    #[must_use]
    pub fn with_removal_hook(mut self, hook: Arc<dyn RegistrationRemovalHook>) -> RegistrationService {
        self.removal_hook = Some(hook);
        self
    }

    /// The JSON-LD processor registrations are expanded and compacted with,
    /// which also resolves the `@context` a registration response names
    /// (clause 5.2.3).
    #[must_use]
    pub const fn engine(&self) -> &JsonLdEngine {
        &self.engine
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
        "andromeda-json-literal",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-compaction",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-request",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-persistence-failure",
    ],
    allow_workspace_dev_dependencies: ["andromeda-ngsi-ld-representation"],
    allow_production_dependencies: [],
}
