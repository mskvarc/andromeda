//! The ports a notifier is built on, as doubles for tests of what it decides
//! before it reads anything: a subscription store that records every slot it
//! is asked for and refuses each, and an entity store that holds nothing.

use andromeda_configuration::schema::request_limits::RequestLimitsConfig;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::{dispatch::TransportDispatcher, http::config::HttpTransportConfig, mqtt::config::MqttPoolConfig};
use andromeda_entity::{
    configured_policy::{filter_complexity_limits, geometry_census_limits},
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    pagination_outcome::PaginationOutcome,
    port::{payload_pages::EntityPayloadPages, query::EntityQueryStore, reader::EntityReader, retrieved_entity::RetrievedEntity, stored_entry::StoredEntry},
    query::EntityQueryService,
    query_outcome::QueryOutcome,
    query_residual::ResidualFilter,
    spatial_acceleration::SpatialAcceleration,
    storage_profile::StorageProfile,
};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    notification::SysAttrsSnapshot,
    subscription::{Subscription, SubscriptionStatus},
};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity::compiled::CompiledEntityQuery,
    filter::language_targeting::LanguageTargeting,
    geo::{admission::GeoQueryAdmission, near_distance_band::NearDistanceBand},
    page_window::PageWindow,
    projection::returned_attributes::ReturnedAttributes,
};
use andromeda_subscription::{
    error::SubscriptionError,
    notification_slot::{NotificationOutcome, NotificationSlot, NotificationSlotGate, OutcomeRecord},
    notifying_since::NotifyingRestart,
    store::{SubscriptionFields, SubscriptionStore},
    stored_subscription::StoredSubscription,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// A subscription store that records the gate of every slot it is asked for
/// and answers each as suppressed, so a notifier that reaches its claim stops
/// there; every other operation answers a saturated store.
#[derive(Default)]
pub(crate) struct ClaimRecordingStore {
    claims: Mutex<Vec<(Identifier, NotificationSlotGate)>>,
}

impl ClaimRecordingStore {
    /// The slots asked for so far, in the order they were asked for.
    pub(crate) fn claims(&self) -> Vec<(Identifier, NotificationSlotGate)> {
        self.claims.lock().expect("the claim record is never poisoned").clone() // clone: the assertion reads a snapshot
    }
}

#[async_trait]
impl SubscriptionStore for ClaimRecordingStore {
    async fn get(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<StoredSubscription>, SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn list(&self, _target: &ContextTarget, _window: &PageWindow) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn delete(&self, _target: &ContextTarget, _id: &Identifier) -> Result<u64, SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn exists(&self, _target: &ContextTarget, _id: &Identifier) -> Result<bool, SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn list_active(&self, _target: &ContextTarget) -> Result<Vec<StoredSubscription>, SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn claim_notification_slot(
        &self,
        _target: &ContextTarget,
        id: &Identifier,
        gate: NotificationSlotGate,
    ) -> Result<NotificationSlot, SubscriptionError> {
        self.claims.lock().expect("the claim record is never poisoned").push((id.clone(), gate)); // clone: the record owns what it saw
        Ok(NotificationSlot::Suppressed)
    }

    async fn record_outcome(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _outcome: NotificationOutcome,
        _record: OutcomeRecord,
    ) -> Result<(), SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn expire_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<Identifier>, SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn find_due_interval_subscriptions(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<StoredSubscription>, SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn next_interval_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn record_interval_tick(&self, _target: &ContextTarget, _id: &Identifier, _cutoff: DateTime<Utc>) -> Result<(), SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn create_subscription(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _fields: &SubscriptionFields<'_>,
        _status: SubscriptionStatus,
    ) -> Result<(), SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn update_subscription(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _fields: &SubscriptionFields<'_>,
        _status: SubscriptionStatus,
        _restart: NotifyingRestart,
    ) -> Result<u64, SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }
}

/// An entity store holding no entity, read through every entity port.
pub(crate) struct NoEntities;

#[async_trait]
impl EntityReader for NoEntities {
    async fn get(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<Arc<EntityState>>, EntityError> {
        Ok(None)
    }

    async fn get_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, RetrievedEntity>, EntityError> {
        Ok(HashMap::new())
    }

    async fn get_snapshot(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<EntitySnapshot>, EntityError> {
        Ok(None)
    }

    async fn get_snapshot_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, EntitySnapshot>, EntityError> {
        Ok(HashMap::new())
    }

    async fn get_with_sys_attrs(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _returned: &ReturnedAttributes<'_>,
    ) -> Result<Option<RetrievedEntity>, EntityError> {
        Ok(None)
    }

    async fn get_sys_attrs(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<SysAttrsSnapshot>, EntityError> {
        Ok(None)
    }

    async fn get_sys_attrs_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, SysAttrsSnapshot>, EntityError> {
        Ok(HashMap::new())
    }
}

#[async_trait]
impl EntityQueryStore for NoEntities {
    fn profile(&self) -> &StorageProfile {
        &StorageProfile::POSTGIS_LTREE
    }

    async fn query_compiled(
        &self,
        _target: &ContextTarget,
        _compiled: &CompiledEntityQuery,
        _window: &PageWindow,
        _count: CountPolicy,
    ) -> Result<QueryOutcome<StoredEntry<RetrievedEntity>>, EntityError> {
        Ok(QueryOutcome {
            rows: Vec::new(),
            unapplied: ResidualFilter::default(),
            pagination: PaginationOutcome::Applied { total: None },
        })
    }

    async fn query_ids(&self, _target: &ContextTarget, _compiled: &CompiledEntityQuery, _window: &PageWindow) -> Result<QueryOutcome<Identifier>, EntityError> {
        Ok(QueryOutcome {
            rows: Vec::new(),
            unapplied: ResidualFilter::default(),
            pagination: PaginationOutcome::Applied { total: None },
        })
    }
}

#[async_trait]
impl EntityPayloadPages for NoEntities {
    async fn get_entities_by_type(
        &self,
        _target: &ContextTarget,
        _entity_type: &EntityTypeName,
        _after: Option<&Identifier>,
        _limit: u16,
    ) -> Result<Vec<StoredEntry<EntityState>>, EntityError> {
        Ok(Vec::new())
    }

    async fn get_all_entities(&self, _target: &ContextTarget, _after: Option<&Identifier>, _limit: u16) -> Result<Vec<StoredEntry<EntityState>>, EntityError> {
        Ok(Vec::new())
    }
}

/// A JSON-LD engine that never fetches a remote context.
pub(crate) fn offline_engine() -> JsonLdEngine {
    JsonLdEngine::builder().allow_remote(false).build().expect("the offline engine builds")
}

/// The dispatcher every transport binding is reached through, with the
/// default configuration.
pub(crate) fn transport() -> Arc<TransportDispatcher> {
    Arc::new(TransportDispatcher::new(HttpTransportConfig::default(), MqttPoolConfig::default()).expect("the dispatcher builds"))
}

/// The entity query service over `entities`, under the default request
/// limits.
pub(crate) fn entity_query(entities: &Arc<NoEntities>) -> Arc<EntityQueryService> {
    Arc::new(
        EntityQueryService::builder()
            .reader(Arc::<NoEntities>::clone(entities))
            .query_store(Arc::<NoEntities>::clone(entities))
            .engine(offline_engine())
            .spatial(SpatialAcceleration::StoreVerified)
            .tenant(TenantRef::UnnamedDefault)
            .remote_linked(None)
            .geo_admission(GeoQueryAdmission::new(
                geometry_census_limits(&RequestLimitsConfig::default()),
                NearDistanceBand::Refused,
            ))
            .language_targeting(LanguageTargeting::Specification)
            .filter_complexity(filter_complexity_limits(&RequestLimitsConfig::default()))
            .build(),
    )
}
