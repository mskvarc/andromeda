//! A fixed in-memory registration store, local-type resolver and geoquery terms for the forwarding tests. Registration matching reads candidates through [`RegistrationStore::find_matching`], which answers the whole fixed list as the superset preselect it is allowed to be and records the specifications it was asked with, and a read by identifier answers the fixed registration of that identifier as stored, expired or not; every other read answers nothing, a creation is refused as an unavailable store, and an update or deletion finds no registration.

use crate::{FederationService, local_entity_type::LocalEntityTypeResolver, local_source::LocalSource};
use andromeda_configuration::schema::federation::FederationConfig;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryPositionLimit};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    context_source_pseudonym::ContextSourcePseudonym,
    entity_type_name::EntityTypeName,
    identifier::Identifier,
};
use andromeda_ngsi_ld_model::{csource_change_revision::CsourceChangeRevision, registration::ContextSourceRegistration};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_query::{
    csr::{compiled::CompiledCsrQuery, expanded_entity_specification::ExpandedEntitySpecification},
    geo::{admission::GeoQueryAdmission, near_distance_band::NearDistanceBand},
    page_window::PageWindow,
};
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
use andromeda_tenancy::{context_source_alias::ContextSourceAliases, tenant_ref::TenantRef};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

/// A forwarding service choosing among `registrations`, holding no entity of
/// its own and resolving only embedded contexts, never the network.
pub(crate) fn offline_service(registrations: Vec<ContextSourceRegistration>) -> FederationService {
    FederationService::new(
        Arc::new(FixedRegistrationStore::new(registrations)),
        Arc::new(NoLocalEntities),
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds"),
        LocalSource::new(
            ContextSourceAliases::new(ContextSourcePseudonym::new("andromeda-test").expect("a valid alias")),
            TenantRef::UnnamedDefault,
        ),
        &FederationConfig::default(),
    )
    .expect("the service builds")
}

/// A forwarding service choosing among `registrations`, holding the one entity
/// of `local_types` whatever its identifier, and resolving only embedded
/// contexts, never the network.
pub(crate) fn service_holding_entity_of(registrations: Vec<ContextSourceRegistration>, local_types: Vec<EntityTypeName>) -> FederationService {
    FederationService::new(
        Arc::new(FixedRegistrationStore::new(registrations)),
        Arc::new(LocalEntityOf(local_types)),
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds"),
        LocalSource::new(
            ContextSourceAliases::new(ContextSourcePseudonym::new("andromeda-test").expect("a valid alias")),
            TenantRef::UnnamedDefault,
        ),
        &FederationConfig::default(),
    )
    .expect("the service builds")
}

/// Geoquery terms no test geometry reaches, refusing a `near` with both
/// distances as clause 4.10 is written.
pub(crate) fn generous_geo_admission() -> GeoQueryAdmission {
    GeoQueryAdmission::new(
        GeometryCensusLimits {
            positions: GeometryPositionLimit::new(NonZeroUsize::MIN.saturating_add(999)),
            values: JsonValueLimit::new(NonZeroUsize::MIN.saturating_add(999)),
        },
        NearDistanceBand::Refused,
    )
}

/// A store holding a fixed registration list.
pub(crate) struct FixedRegistrationStore {
    /// The registrations every matching query answers.
    registrations: Vec<ContextSourceRegistration>,
    /// How many forwarding outcomes were recorded on the registrations.
    recorded_outcomes: AtomicUsize,
    /// The clause 5.12 specifications each matching query asked with, in
    /// their debug rendering, one entry per query.
    asked_specifications: Mutex<Vec<Vec<String>>>,
}

impl FixedRegistrationStore {
    /// A store holding `registrations`.
    pub(crate) const fn new(registrations: Vec<ContextSourceRegistration>) -> FixedRegistrationStore {
        FixedRegistrationStore {
            registrations,
            recorded_outcomes: AtomicUsize::new(0),
            asked_specifications: Mutex::new(Vec::new()),
        }
    }

    /// How many forwarding outcomes were recorded on the registrations.
    pub(crate) fn recorded_outcomes(&self) -> usize {
        self.recorded_outcomes.load(Ordering::SeqCst)
    }

    /// The clause 5.12 specifications every matching query asked with, one
    /// entry per query, each specification in its debug rendering.
    pub(crate) fn asked_specifications(&self) -> Vec<Vec<String>> {
        self.asked_specifications.lock().expect("the record is not poisoned").clone() // clone: the caller reads a snapshot of the record
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
    ) -> BoxFuture<'async_trait, Result<WrittenRegistration, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
    {
        Box::pin(async { Err(RegistrationError::StoreUnavailable) })
    }

    fn get<'life0, 'life1, 'async_trait>(
        &'life0 self,
        id: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        let registration = self.registrations.iter().find(|registration| &registration.id == id).cloned(); // clone: the read answers its own copy of the fixed registration
        Box::pin(async { Ok(registration) })
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
    ) -> BoxFuture<'async_trait, Result<RegistrationMutation<WrittenRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
    {
        Box::pin(async { Ok(RegistrationMutation::Missing) })
    }

    fn find_matching<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        entities: &'life1 [ExpandedEntitySpecification<'life2>],
    ) -> BoxFuture<'async_trait, Result<Vec<ContextSourceRegistration>, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
    {
        self.asked_specifications
            .lock()
            .expect("the record is not poisoned")
            .push(entities.iter().map(|specification| format!("{specification:?}")).collect());
        let registrations = self.registrations.clone(); // clone: every query answers its own copy of the fixed list
        Box::pin(async { Ok(registrations) })
    }

    fn find_overlapping<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        _entity_id: Option<&'life1 Identifier>,
        _attributes: &'life2 [AttributeName],
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

    fn delete<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _id: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<RegistrationMutation<CsourceChangeRevision>, RegistrationError>>
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
        self.recorded_outcomes.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}

/// A resolver for which no entity is held locally, so matching falls back to
/// the entity identifier.
pub(crate) struct NoLocalEntities;

impl LocalEntityTypeResolver for NoLocalEntities {
    fn local_entity_types<'resolver>(
        &'resolver self,
        _target: &'resolver ContextTarget,
        _id: &'resolver Identifier,
    ) -> BoxFuture<'resolver, Vec<EntityTypeName>> {
        Box::pin(async { Vec::new() })
    }
}

/// A resolver for which every entity is held locally with the same types.
pub(crate) struct LocalEntityOf(Vec<EntityTypeName>);

impl LocalEntityTypeResolver for LocalEntityOf {
    fn local_entity_types<'resolver>(
        &'resolver self,
        _target: &'resolver ContextTarget,
        _id: &'resolver Identifier,
    ) -> BoxFuture<'resolver, Vec<EntityTypeName>> {
        let types = self.0.clone(); // clone: the answer owns the types the fixture holds
        Box::pin(async { types })
    }
}
