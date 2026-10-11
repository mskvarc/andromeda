//! The registrations a re-assembly test finds the other holders of an entity
//! through.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{csource_change_revision::CsourceChangeRevision, registration::ContextSourceRegistration};
use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrQuery, page_window::PageWindow};
use andromeda_registration::{
    error::RegistrationError,
    resolved_update::ResolvedCsrUpdate,
    store::{
        RegistrationStore,
        create_params::CreateCsrParams,
        expired_deletion::ExpiredDeletion,
        forwarding_outcome::ForwardingOutcome,
        mutation::RegistrationMutation,
        page::RegistrationPage,
        written_registration::WrittenRegistration,
    },
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A registration store that answers every matching query with its fixed list,
/// except for the first few, which it fails. A creation is refused as an
/// unavailable store, and an update or deletion finds no registration.
pub(crate) struct ScriptedRegistrations {
    /// The registrations every matching query answers.
    registrations: Vec<ContextSourceRegistration>,
    /// How many of the following matching queries still fail.
    failures_left: AtomicUsize,
}

impl ScriptedRegistrations {
    /// A store answering matching queries with `registrations`, the first
    /// `failures` of them with a store failure instead.
    pub(crate) const fn new(registrations: Vec<ContextSourceRegistration>, failures: usize) -> ScriptedRegistrations {
        ScriptedRegistrations {
            registrations,
            failures_left: AtomicUsize::new(failures),
        }
    }
}

#[async_trait]
impl RegistrationStore for ScriptedRegistrations {
    async fn delete_expired(&self, _cutoff: DateTime<Utc>) -> Result<ExpiredDeletion, RegistrationError> {
        Ok(ExpiredDeletion {
            revision: CsourceChangeRevision::from(1),
            registrations: Vec::new(),
        })
    }

    async fn find_expired(&self, _cutoff: DateTime<Utc>) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        Ok(Vec::new())
    }

    async fn exists(&self, _id: &Identifier) -> Result<bool, RegistrationError> {
        Ok(false)
    }

    async fn create(&self, _id: &Identifier, _params: &CreateCsrParams) -> Result<WrittenRegistration, RegistrationError> {
        Err(RegistrationError::StoreUnavailable)
    }

    async fn get(&self, _id: &Identifier) -> Result<Option<ContextSourceRegistration>, RegistrationError> {
        Ok(None)
    }

    async fn query_compiled(&self, _compiled: &CompiledCsrQuery, _window: &PageWindow) -> Result<RegistrationPage, RegistrationError> {
        Ok(RegistrationPage {
            registrations: Vec::new(),
            total: 0,
        })
    }

    async fn update(&self, _id: &Identifier, _values: &ResolvedCsrUpdate) -> Result<RegistrationMutation<WrittenRegistration>, RegistrationError> {
        Ok(RegistrationMutation::Missing)
    }

    async fn find_matching(&self, _entity_type: Option<&IriBuf>, _entity_id: Option<&Identifier>) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        let failing = self
            .failures_left
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| left.checked_sub(1))
            .is_ok();
        if failing {
            return Err(RegistrationError::StoreUnavailable);
        }
        Ok(self.registrations.clone()) // clone: every query answers its own copy of the fixed list
    }

    async fn find_overlapping(&self, _entity_id: Option<&Identifier>, _attributes: &[IriBuf]) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        Ok(Vec::new())
    }

    async fn find_all_active(&self, _after: Option<&Identifier>, _limit: u32) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        Ok(Vec::new())
    }

    async fn delete(&self, _id: &Identifier) -> Result<RegistrationMutation<CsourceChangeRevision>, RegistrationError> {
        Ok(RegistrationMutation::Missing)
    }

    async fn record_forwarding_outcome(&self, _id: &Identifier, _outcome: ForwardingOutcome, _at: DateTime<Utc>) -> Result<(), RegistrationError> {
        Ok(())
    }
}
