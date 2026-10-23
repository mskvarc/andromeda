//! One matching Building subscription, as the subscription cache learns it
//! from its store.
//!
//! The durable-match tests of every mutation path and the side-effect drain's
//! durable settlement tests match against the same subscription, so its stored
//! form and the cache loaded with it live here rather than in either harness.

use andromeda_context_commit::subscription_key::SubscriptionKey;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::subscription::{Subscription, SubscriptionStatus};
use andromeda_ngsi_ld_query::page_window::PageWindow;
use andromeda_subscription::{
    cache::SubscriptionCache,
    error::SubscriptionError,
    notification_slot::{NotificationOutcome, NotificationSlot, NotificationSlotGate, OutcomeRecord},
    store::{SubscriptionFields, SubscriptionStore},
    stored_subscription::StoredSubscription,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::json;
use std::sync::Arc;

/// The expanded Building type both the fixture entities and the subscription
/// selector carry.
pub(crate) const BUILDING_TYPE_IRI: &str = "https://uri.etsi.org/ngsi-ld/default-context/Building";
/// The subscription every Building change is matched against.
const SUBSCRIPTION_ID: &str = "urn:ngsi-ld:Subscription:durable-test";
/// The surrogate key the fixture subscription is stored under.
const SUBSCRIPTION_KEY: i64 = 1;
/// A second subscription matching every Building change alike.
const TWIN_SUBSCRIPTION_ID: &str = "urn:ngsi-ld:Subscription:durable-twin";
/// The surrogate key the second subscription is stored under.
const TWIN_SUBSCRIPTION_KEY: i64 = 2;
/// Every trigger clause 5.2.12 defines, so the subscription matches every
/// change of the Building.
const EVERY_TRIGGER: &[&str] = &[
    "entityCreated",
    "entityUpdated",
    "entityDeleted",
    "attributeCreated",
    "attributeUpdated",
    "attributeDeleted",
];

/// A subscription cache for the live branch holding the one Building
/// subscription.
pub(crate) async fn building_subscription_cache() -> Arc<SubscriptionCache> {
    subscription_cache_of(vec![(SUBSCRIPTION_ID, SUBSCRIPTION_KEY)], EVERY_TRIGGER).await
}

/// A subscription cache for the live branch holding one Building
/// subscription listening for `entityUpdated` alone, which Table 5.2.12-1
/// defines to include `attributeDeleted`.
pub(crate) async fn entity_updated_building_subscription_cache() -> Arc<SubscriptionCache> {
    subscription_cache_of(vec![(SUBSCRIPTION_ID, SUBSCRIPTION_KEY)], &["entityUpdated"]).await
}

/// A subscription cache for the live branch holding two Building
/// subscriptions, so one change matches twice.
pub(crate) async fn twin_building_subscription_cache() -> Arc<SubscriptionCache> {
    subscription_cache_of(
        vec![(SUBSCRIPTION_ID, SUBSCRIPTION_KEY), (TWIN_SUBSCRIPTION_ID, TWIN_SUBSCRIPTION_KEY)],
        EVERY_TRIGGER,
    )
    .await
}

/// A subscription cache for the live branch loaded with the Building
/// subscriptions `subscriptions` identifies, each by its identifier and key,
/// all listening for `triggers`.
async fn subscription_cache_of(subscriptions: Vec<(&'static str, i64)>, triggers: &'static [&'static str]) -> Arc<SubscriptionCache> {
    let cache = Arc::new(SubscriptionCache::new(TenantRef::UnnamedDefault));
    cache
        .reload(&ContextTarget::Live, &FixtureSubscriptionStore { subscriptions, triggers })
        .await
        .expect("the fixture subscriptions reload");
    cache
}

/// A subscription store whose only exercised method is `list_active`, which the
/// cache reloads once to learn the matching Building subscriptions.
struct FixtureSubscriptionStore {
    /// The identifier and key of each Building subscription `list_active`
    /// answers with.
    subscriptions: Vec<(&'static str, i64)>,
    /// The triggers every one of them listens for.
    triggers: &'static [&'static str],
}

fn unused_store_call(method: &str) -> SubscriptionError {
    panic!("FixtureSubscriptionStore::{method} is not exercised by the notification tests")
}

/// Build a stored Building subscription the cache matches against, identified
/// by `id`, stored under `key` and listening for `triggers`.
///
/// It is stored in expanded form (its selector carries the expanded Building
/// IRI) and is created in the past so the clause 5.2.12 `createdAt` guard
/// admits the tests' events.
fn stored_building_subscription(id: &str, key: i64, triggers: &[&str]) -> StoredSubscription {
    let subscription = serde_json::from_value::<Subscription>(json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": BUILDING_TYPE_IRI}],
        "notificationTrigger": triggers,
        "notification": {"endpoint": {"uri": "http://localhost/notify"}},
        "status": "active",
        "createdAt": "2020-01-01T00:00:00Z",
        "modifiedAt": "2020-01-01T00:00:00Z"
    }))
    .expect("a valid fixture subscription");
    let key = SubscriptionKey::try_from(key).expect("a valid fixture subscription key");
    StoredSubscription { key, subscription }
}

/// The fixture subscription's surrogate key, as the store would have assigned it.
pub(crate) fn subscription_key() -> SubscriptionKey {
    SubscriptionKey::try_from(SUBSCRIPTION_KEY).expect("the fixture subscription key is positive")
}

#[async_trait]
impl SubscriptionStore for FixtureSubscriptionStore {
    async fn get(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<StoredSubscription>, SubscriptionError> {
        Err(unused_store_call("get"))
    }

    async fn list(&self, _target: &ContextTarget, _window: &PageWindow) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
        Err(unused_store_call("list"))
    }

    async fn delete(&self, _target: &ContextTarget, _id: &Identifier) -> Result<u64, SubscriptionError> {
        Err(unused_store_call("delete"))
    }

    async fn exists(&self, _target: &ContextTarget, _id: &Identifier) -> Result<bool, SubscriptionError> {
        Err(unused_store_call("exists"))
    }

    async fn list_active(&self, _target: &ContextTarget) -> Result<Vec<StoredSubscription>, SubscriptionError> {
        Ok(self
            .subscriptions
            .iter()
            .map(|(id, key)| stored_building_subscription(id, *key, self.triggers))
            .collect())
    }

    async fn claim_notification_slot(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _gate: NotificationSlotGate,
    ) -> Result<NotificationSlot, SubscriptionError> {
        Err(unused_store_call("claim_notification_slot"))
    }

    async fn record_outcome(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _outcome: NotificationOutcome,
        _record: OutcomeRecord,
    ) -> Result<(), SubscriptionError> {
        Err(unused_store_call("record_outcome"))
    }

    async fn expire_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<Identifier>, SubscriptionError> {
        Err(unused_store_call("expire_due"))
    }

    async fn find_due_interval_subscriptions(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<StoredSubscription>, SubscriptionError> {
        Err(unused_store_call("find_due_interval_subscriptions"))
    }

    async fn next_interval_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, SubscriptionError> {
        Err(unused_store_call("next_interval_due"))
    }

    async fn record_interval_tick(&self, _target: &ContextTarget, _id: &Identifier, _cutoff: DateTime<Utc>) -> Result<(), SubscriptionError> {
        Err(unused_store_call("record_interval_tick"))
    }

    async fn create_subscription(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _fields: &SubscriptionFields<'_>,
        _status: SubscriptionStatus,
    ) -> Result<(), SubscriptionError> {
        Err(unused_store_call("create_subscription"))
    }

    async fn update_subscription(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _fields: &SubscriptionFields<'_>,
        _status: SubscriptionStatus,
    ) -> Result<u64, SubscriptionError> {
        Err(unused_store_call("update_subscription"))
    }
}
