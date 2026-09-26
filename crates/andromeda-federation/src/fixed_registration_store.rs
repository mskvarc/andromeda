//! A fixed in-memory registration store and local-type resolver for the
//! forwarding tests.
//!
//! Registration matching only ever reads candidates through
//! [`RegistrationStore::find_matching`], which answers the whole fixed list as
//! the superset preselect it is allowed to be; every other call answers
//! nothing.

use crate::local_entity_type::LocalEntityTypeResolver;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrQuery, pagination::Pagination};
use andromeda_registration::{
    error::RegistrationError,
    resolved_update::ResolvedCsrUpdate,
    store::{RegistrationStore, create_params::CreateCsrParams, forwarding_outcome::ForwardingOutcome, mutation::RegistrationMutation, page::RegistrationPage},
};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use iri_rs::IriBuf;

/// A store holding a fixed registration list.
pub(crate) struct FixedRegistrationStore {
    /// The registrations every matching query answers.
    registrations: Vec<ContextSourceRegistration>,
}

impl FixedRegistrationStore {
    /// A store holding `registrations`.
    pub(crate) const fn new(registrations: Vec<ContextSourceRegistration>) -> FixedRegistrationStore {
        FixedRegistrationStore { registrations }
    }
}

impl RegistrationStore for FixedRegistrationStore {
    fn delete_expired<'life0, 'async_trait>(
        &'life0 self,
        _cutoff: DateTime<Utc>,
    ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
    {
        Box::pin(async { Ok(Vec::new()) })
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

    fn exists<'life0, 'life1, 'async_trait>(&'life0 self, _id: &'life1 Uri) -> BoxFuture<'async_trait, Result<bool, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        Box::pin(async { Ok(false) })
    }

    fn create<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        _id: &'life1 Uri,
        _params: &'life2 CreateCsrParams,
    ) -> BoxFuture<'async_trait, Result<(), RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
    {
        Box::pin(async { Ok(()) })
    }

    fn get<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _id: &'life1 Uri,
    ) -> BoxFuture<'async_trait, Result<Option<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        Box::pin(async { Ok(None) })
    }

    fn list<'life0, 'async_trait>(&'life0 self, _pagination: Pagination) -> BoxFuture<'async_trait, Result<RegistrationPage, RegistrationError>>
    where
        'life0: 'async_trait,
    {
        Box::pin(async {
            Ok(RegistrationPage {
                registrations: Vec::new(),
                total: 0,
            })
        })
    }

    fn query_compiled<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _compiled: &'life1 CompiledCsrQuery,
    ) -> BoxFuture<'async_trait, Result<RegistrationPage, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        Box::pin(async {
            Ok(RegistrationPage {
                registrations: Vec::new(),
                total: 0,
            })
        })
    }

    fn update<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        _id: &'life1 Uri,
        _values: &'life2 ResolvedCsrUpdate,
    ) -> BoxFuture<'async_trait, Result<RegistrationMutation, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
    {
        Box::pin(async { Ok(RegistrationMutation::Missing) })
    }

    fn find_matching<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        _entity_type: Option<&'life1 IriBuf>,
        _entity_id: Option<&'life2 Uri>,
    ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
    {
        let registrations = self.registrations.clone(); // clone: every query answers its own copy of the fixed list
        Box::pin(async { Ok(registrations) })
    }

    fn find_overlapping<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        _entity_id: Option<&'life1 Uri>,
        _attributes: &'life2 [IriBuf],
    ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
    {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn find_all_active<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _after: Option<&'life1 Uri>,
        _limit: u32,
    ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn delete<'life0, 'life1, 'async_trait>(&'life0 self, _id: &'life1 Uri) -> BoxFuture<'async_trait, Result<RegistrationMutation, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        Box::pin(async { Ok(RegistrationMutation::Missing) })
    }

    fn record_forwarding_outcome<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _id: &'life1 Uri,
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

/// A resolver for which no entity is held locally, so matching falls back to
/// the entity identifier.
pub(crate) struct NoLocalEntities;

impl LocalEntityTypeResolver for NoLocalEntities {
    fn local_entity_types<'resolver>(&'resolver self, _target: &'resolver ContextTarget, _id: &'resolver Uri) -> BoxFuture<'resolver, Vec<IriBuf>> {
        Box::pin(async { Vec::new() })
    }
}
