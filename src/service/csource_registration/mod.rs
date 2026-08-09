//! Context-source registration policy (ETSI GS CIM 009 clause 5.10).

use crate::{event::csr_change_event::CsrChangeEvent, service::csource_registration::store::RegistrationStore};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::debug;

mod create;
mod delete;
mod identifier;
mod information_expansion;
mod payload;
mod query;
mod retrieve;
/// The registration store port, and the parameter shapes it takes.
pub mod store;
mod update;
mod validation;

/// Service for managing NGSI-LD context source registrations.
pub struct CsourceRegistrationService {
    repo: Arc<dyn RegistrationStore>,
    engine: JsonLdEngine,
    csr_change_tx: Option<broadcast::Sender<CsrChangeEvent>>,
}

impl CsourceRegistrationService {
    /// Create a context-source registration service backed by the supplied
    /// store and JSON-LD processor.
    pub fn new(repo: Arc<dyn RegistrationStore>, engine: JsonLdEngine, csr_change_tx: Option<broadcast::Sender<CsrChangeEvent>>) -> CsourceRegistrationService {
        debug!("Context source registration service created");
        CsourceRegistrationService { repo, engine, csr_change_tx }
    }

    fn emit_csr_change(&self, event: CsrChangeEvent) {
        if let Some(ref sender) = self.csr_change_tx {
            let _ = sender.send(event);
        }
    }
}
