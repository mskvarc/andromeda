//! A fixed in-memory [`RegistrationStore`] for policy unit tests.

use crate::{
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
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    csource_change_revision::CsourceChangeRevision,
    registration::{ContextSourceRegistration, info::RegistrationInfo},
};
use andromeda_ngsi_ld_query::{
    csr::{compiled::CompiledCsrQuery, expanded_entity_specification::ExpandedEntitySpecification},
    page_window::PageWindow,
};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use iri_rs::IriBuf;
use serde_json::json;
use std::sync::Mutex;

/// A store holding a fixed registration list and recording the updates it is
/// asked to apply.
///
/// `query_compiled` answers the page a window cuts from every registration in
/// identifier order, as a superset preselect is allowed to, leaving the
/// clause 5.10.2.5 trim to the policy layer;
/// `get` answers the registration carrying the requested identifier;
/// `find_overlapping` returns every registration, as a superset preselect is
/// allowed to; `create` answers the registration it was handed as written;
/// `update` records the resolved `information` it received so a test can
/// assert what the policy layer would have written, and answers the held
/// registration carrying it; `delete` records
/// the identifier it was asked to remove, so a test can tell what happened
/// before the row went from what happened after.
pub(crate) struct FixedStore {
    /// The registrations the store holds.
    registrations: Vec<ContextSourceRegistration>,
    /// The `information` of every update applied, in order.
    updated_information: Mutex<Vec<Vec<RegistrationInfo>>>,
    /// Every identifier deleted, in order.
    deleted: Mutex<Vec<Identifier>>,
}

impl FixedStore {
    /// A store holding `registrations`.
    pub(crate) fn new(registrations: Vec<ContextSourceRegistration>) -> FixedStore {
        FixedStore {
            registrations,
            updated_information: Mutex::new(Vec::new()),
            deleted: Mutex::new(Vec::new()),
        }
    }

    /// Every identifier deleted so far.
    pub(crate) fn deleted(&self) -> Vec<Identifier> {
        self.deleted.lock().expect("deletion log").clone() // clone: the test reads what the fake recorded
    }

    /// The `information` of every update applied so far.
    pub(crate) fn updated_information(&self) -> Vec<Vec<RegistrationInfo>> {
        self.updated_information.lock().expect("update log").clone() // clone: the test reads what the fake recorded
    }
}

impl RegistrationStore for FixedStore {
    fn delete_expired<'life0, 'async_trait>(&'life0 self, _cutoff: DateTime<Utc>) -> BoxFuture<'async_trait, Result<ExpiredDeletion, RegistrationError>>
    where
        'life0: 'async_trait,
    {
        Box::pin(async {
            Ok(ExpiredDeletion {
                revision: CsourceChangeRevision::from(1),
                registrations: Vec::new(),
            })
        })
    }

    fn find_expired<'life0, 'async_trait>(
        &'life0 self,
        _cutoff: DateTime<Utc>,
    ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
    {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn exists<'life0, 'life1, 'async_trait>(&'life0 self, _id: &'life1 Identifier) -> BoxFuture<'async_trait, Result<bool, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        Box::pin(async { Ok(false) })
    }

    fn create<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        id: &'life1 Identifier,
        params: &'life2 CreateCsrParams,
    ) -> BoxFuture<'async_trait, Result<WrittenRegistration, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
    {
        let mut created = registration(id.as_str(), "inclusive", &json!(params.information));
        created.access.mode = Some(params.mode);
        Box::pin(async move {
            Ok(WrittenRegistration {
                revision: CsourceChangeRevision::from(1),
                registration: created,
            })
        })
    }

    fn get<'life0, 'life1, 'async_trait>(
        &'life0 self,
        id: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        let found = self.registrations.iter().find(|registration| registration.id == *id).cloned(); // clone: the port hands out owned registrations
        Box::pin(async { Ok(found) })
    }

    fn query_compiled<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        _compiled: &'life1 CompiledCsrQuery,
        window: &'life2 PageWindow,
    ) -> BoxFuture<'async_trait, Result<RegistrationPage, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
    {
        let mut sorted = self.registrations.clone(); // clone: the page owns the registrations it hands out
        sorted.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        let total = u64::try_from(sorted.len()).unwrap_or(u64::MAX);
        let registrations = window.cut(sorted, |registration| registration.id.as_str());
        Box::pin(async move { Ok(RegistrationPage { registrations, total }) })
    }

    fn update<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        id: &'life1 Identifier,
        values: &'life2 ResolvedCsrUpdate,
    ) -> BoxFuture<'async_trait, Result<RegistrationMutation<WrittenRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
    {
        self.updated_information.lock().expect("update log").push(values.information.clone()); // clone: the fake keeps a record the caller still owns
        let mutation = match self.registrations.iter().find(|registration| registration.id == *id) {
            Some(held) => {
                let mut updated = held.clone(); // clone: the held registration stays as the fake was built
                updated.coverage.information = values.information.clone(); // clone: the written registration owns what the caller still holds
                updated.access.mode = Some(values.mode);
                RegistrationMutation::Applied(WrittenRegistration {
                    revision: CsourceChangeRevision::from(1),
                    registration: updated,
                })
            }
            None => RegistrationMutation::Missing,
        };
        Box::pin(async move { Ok(mutation) })
    }

    fn find_matching<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _entities: ExpandedEntitySpecification<'life1>,
    ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn find_overlapping<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        _entity_id: Option<&'life1 Identifier>,
        _attributes: &'life2 [IriBuf],
    ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
    {
        Box::pin(async { Ok(self.registrations.clone()) }) // clone: the fake hands out owned registrations like the port requires
    }

    fn find_all_active<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _after: Option<&'life1 Identifier>,
        _limit: u32,
    ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn delete<'life0, 'life1, 'async_trait>(
        &'life0 self,
        id: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<RegistrationMutation<CsourceChangeRevision>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        let mutation = if self.registrations.iter().any(|registration| registration.id == *id) {
            self.deleted.lock().expect("deletion log").push(id.clone()); // clone: the fake keeps a record the caller still owns
            RegistrationMutation::Applied(CsourceChangeRevision::from(1))
        } else {
            RegistrationMutation::Missing
        };
        Box::pin(async move { Ok(mutation) })
    }

    fn record_forwarding_outcome<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _id: &'life1 Identifier,
        _outcome: ForwardingOutcome,
        _at: DateTime<Utc>,
    ) -> BoxFuture<'async_trait, Result<(), RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        Box::pin(async { Ok(()) })
    }
}

/// A registration with the given identifier, mode and `information`, as the
/// store would hand it back.
pub(crate) fn registration(id: &str, mode: &str, information: &serde_json::Value) -> ContextSourceRegistration {
    serde_json::from_value(json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "endpoint": "https://peer.example.org",
        "information": information,
        "mode": mode,
        "status": "ok",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-01T00:00:00Z"
    }))
    .expect("valid registration")
}
