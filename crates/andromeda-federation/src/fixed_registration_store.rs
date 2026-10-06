//! A fixed in-memory registration store and local-type resolver for the
//! forwarding tests.
//!
//! Registration matching only ever reads candidates through
//! [`RegistrationStore::find_matching`], which answers the whole fixed list as
//! the superset preselect it is allowed to be; every other call answers
//! nothing.

use crate::{FederationService, config::FederationConfig, local_entity_type::LocalEntityTypeResolver};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier};
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
    },
};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use iri_rs::IriBuf;
use std::sync::Arc;

/// A forwarding service choosing among `registrations`, holding no entity of
/// its own and resolving only embedded contexts, never the network.
pub(crate) fn offline_service(registrations: Vec<ContextSourceRegistration>) -> FederationService {
    FederationService::new(
        Arc::new(FixedRegistrationStore::new(registrations)),
        Arc::new(NoLocalEntities),
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds"),
        ContextSourcePseudonym::new("andromeda-test").expect("a valid alias"),
        &FederationConfig::default(),
    )
    .expect("the service builds")
}

/// A forwarding service choosing among `registrations`, holding the one entity
/// of `local_types` whatever its identifier, and resolving only embedded
/// contexts, never the network.
pub(crate) fn service_holding_entity_of(registrations: Vec<ContextSourceRegistration>, local_types: Vec<IriBuf>) -> FederationService {
    FederationService::new(
        Arc::new(FixedRegistrationStore::new(registrations)),
        Arc::new(LocalEntityOf(local_types)),
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds"),
        ContextSourcePseudonym::new("andromeda-test").expect("a valid alias"),
        &FederationConfig::default(),
    )
    .expect("the service builds")
}

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
        _id: &'life1 Identifier,
        _params: &'life2 CreateCsrParams,
    ) -> BoxFuture<'async_trait, Result<CsourceChangeRevision, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
    {
        Box::pin(async { Ok(CsourceChangeRevision::from(1)) })
    }

    fn get<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _id: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        Box::pin(async { Ok(None) })
    }

    fn query_compiled<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        _compiled: &'life1 CompiledCsrQuery,
        _window: &'life2 PageWindow,
    ) -> BoxFuture<'async_trait, Result<RegistrationPage, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
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
        _id: &'life1 Identifier,
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
        _entity_id: Option<&'life2 Identifier>,
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
        _entity_id: Option<&'life1 Identifier>,
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
        _after: Option<&'life1 Identifier>,
        _limit: u32,
    ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn delete<'life0, 'life1, 'async_trait>(&'life0 self, _id: &'life1 Identifier) -> BoxFuture<'async_trait, Result<RegistrationMutation, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        Box::pin(async { Ok(RegistrationMutation::Missing) })
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

/// A resolver for which no entity is held locally, so matching falls back to
/// the entity identifier.
pub(crate) struct NoLocalEntities;

impl LocalEntityTypeResolver for NoLocalEntities {
    fn local_entity_types<'resolver>(&'resolver self, _target: &'resolver ContextTarget, _id: &'resolver Identifier) -> BoxFuture<'resolver, Vec<IriBuf>> {
        Box::pin(async { Vec::new() })
    }
}

/// A resolver for which every entity is held locally with the same types.
pub(crate) struct LocalEntityOf(Vec<IriBuf>);

impl LocalEntityTypeResolver for LocalEntityOf {
    fn local_entity_types<'resolver>(&'resolver self, _target: &'resolver ContextTarget, _id: &'resolver Identifier) -> BoxFuture<'resolver, Vec<IriBuf>> {
        let types = self.0.clone(); // clone: the answer owns the types the fixture holds
        Box::pin(async { types })
    }
}
