//! Context-source registration policy (ETSI GS CIM 009 clause 5.9).

use crate::{entity_probe::RegisteredEntityProbe, registration_census::RegistrationCensus, removal_hook::RegistrationRemovalHook, store::RegistrationStore};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_model::notification::CsrChangeEvent;
use andromeda_ngsi_ld_query::geo::admission::GeoQueryAdmission;
use futures::future::BoxFuture;
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
mod member_presence;
mod mode_rules;
pub mod query;
/// The Create and Update Context Source Registration request body.
pub mod registration_body;
mod registration_body_reading;
/// How many registrations a tenant holds.
pub mod registration_census;
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
    /// Persists and reads the registrations.
    repo: Arc<dyn RegistrationStore>,
    /// How many registrations the tenant holds, counted as they are created
    /// and deleted.
    census: RegistrationCensus,
    /// Looks into the local entity store when a proxied registration is admitted.
    entity_probe: Arc<dyn RegisteredEntityProbe>,
    /// Expands and compacts registration terms and resolves the `@context` a response names.
    engine: JsonLdEngine,
    /// Receives each committed registration change; `None` publishes nothing.
    change_sink: Option<Arc<dyn RegistrationChangeSink>>,
    /// The distributed side of removing a registration; `None` until one is attached.
    removal_hook: Option<Arc<dyn RegistrationRemovalHook>>,
    /// What a discovery geo-query is admitted under; its limits also bound
    /// a geometry the registration writes.
    geo_admission: GeoQueryAdmission,
}

/// Publishes registration lifecycle changes without coupling policy to a runtime transport.
pub trait RegistrationChangeSink: Send + Sync + 'static {
    /// Publish one committed registration change.
    ///
    /// The returned future resolves once the change is handed over, which may
    /// wait while the consumer catches up: clause 5.11.7 owes a notification
    /// for every change of a matching registration, so none is dropped. The
    /// change has already committed, so dropping the future does not withdraw
    /// it.
    fn publish(&self, event: CsrChangeEvent) -> BoxFuture<'static, ()>;
}

impl RegistrationService {
    /// Create a context-source registration service backed by the supplied
    /// store and the census of what it holds, the local-entity probe
    /// proxied-registration admission consults, and JSON-LD processor.
    pub const fn new(
        repo: Arc<dyn RegistrationStore>,
        census: RegistrationCensus,
        entity_probe: Arc<dyn RegisteredEntityProbe>,
        engine: JsonLdEngine,
        change_sink: Option<Arc<dyn RegistrationChangeSink>>,
        geo_admission: GeoQueryAdmission,
    ) -> RegistrationService {
        RegistrationService {
            repo,
            census,
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

    /// Hand `event` to the change sink, waiting while its consumer catches up; does
    /// nothing without a sink.
    async fn emit_csr_change(&self, event: CsrChangeEvent) {
        if let Some(ref sink) = self.change_sink {
            sink.publish(event).await;
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
