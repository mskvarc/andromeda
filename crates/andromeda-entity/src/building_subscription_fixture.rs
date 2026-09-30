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
use andromeda_ngsi_ld_query::cursor::PageCursor;
use andromeda_subscription::{
    cache::SubscriptionCache,
    error::SubscriptionError,
    notification_slot::{NotificationSlot, NotificationSlotGate},
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

/// A subscription cache for the live branch holding the one Building
/// subscription.
pub(crate) async fn building_subscription_cache() -> Arc<SubscriptionCache> {
    let cache = Arc::new(SubscriptionCache::new(TenantRef::UnnamedDefault));
    cache
        .reload(&ContextTarget::Live, &SingleSubscriptionStore)
        .await
        .expect("the fixture subscription reloads");
    cache
}

/// A subscription store whose only exercised method is `list_active`, which the
/// cache reloads once to learn the one matching Building subscription.
struct SingleSubscriptionStore;

fn unused_store_call(method: &str) -> SubscriptionError {
    panic!("SingleSubscriptionStore::{method} is not exercised by the notification tests")
}

/// Build the one stored Building subscription the cache matches against.
///
/// It is stored in expanded form (its selector carries the expanded Building
/// IRI), explicitly requests all six triggers, and is created in the past so
/// the clause 5.2.12 `createdAt` guard admits the tests' events.
fn stored_building_subscription() -> StoredSubscription {
    let subscription = serde_json::from_value::<Subscription>(json!({
        "id": SUBSCRIPTION_ID,
        "type": "Subscription",
        "entities": [{"type": BUILDING_TYPE_IRI}],
        "notificationTrigger": ["entityCreated", "entityUpdated", "entityDeleted", "attributeCreated", "attributeUpdated", "attributeDeleted"],
        "notification": {"endpoint": {"uri": "http://localhost/notify"}},
        "status": "active",
        "createdAt": "2020-01-01T00:00:00Z",
        "modifiedAt": "2020-01-01T00:00:00Z"
    }))
    .expect("a valid fixture subscription");
    let key = SubscriptionKey::try_from(SUBSCRIPTION_KEY).expect("a valid fixture subscription key");
    StoredSubscription { key, subscription }
}

/// The fixture subscription's surrogate key, as the store would have assigned it.
pub(crate) fn subscription_key() -> SubscriptionKey {
    SubscriptionKey::try_from(SUBSCRIPTION_KEY).expect("the fixture subscription key is positive")
}

#[async_trait]
impl SubscriptionStore for SingleSubscriptionStore {
    async fn get(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<StoredSubscription>, SubscriptionError> {
        Err(unused_store_call("get"))
    }

    async fn list(&self, _target: &ContextTarget, _limit: Option<i64>, _offset: Option<i64>) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
        Err(unused_store_call("list"))
    }

    async fn list_keyset(&self, _target: &ContextTarget, _cursor: Option<&PageCursor>, _limit: i64) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
        Err(unused_store_call("list_keyset"))
    }

    async fn delete(&self, _target: &ContextTarget, _id: &Identifier) -> Result<u64, SubscriptionError> {
        Err(unused_store_call("delete"))
    }

    async fn exists(&self, _target: &ContextTarget, _id: &Identifier) -> Result<bool, SubscriptionError> {
        Err(unused_store_call("exists"))
    }

    async fn list_active(&self, _target: &ContextTarget) -> Result<Vec<StoredSubscription>, SubscriptionError> {
        Ok(vec![stored_building_subscription()])
    }

    async fn claim_notification_slot(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _gate: NotificationSlotGate,
    ) -> Result<NotificationSlot, SubscriptionError> {
        Err(unused_store_call("claim_notification_slot"))
    }

    async fn mark_success(&self, _target: &ContextTarget, _id: &Identifier) -> Result<(), SubscriptionError> {
        Err(unused_store_call("mark_success"))
    }

    async fn mark_failure(&self, _target: &ContextTarget, _id: &Identifier) -> Result<(), SubscriptionError> {
        Err(unused_store_call("mark_failure"))
    }

    async fn expire_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<Identifier>, SubscriptionError> {
        Err(unused_store_call("expire_due"))
    }

    async fn find_due_interval_subscriptions(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<Subscription>, SubscriptionError> {
        Err(unused_store_call("find_due_interval_subscriptions"))
    }

    async fn next_interval_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, SubscriptionError> {
        Err(unused_store_call("next_interval_due"))
    }

    async fn advance_interval_anchor(&self, _target: &ContextTarget, _id: &Identifier, _cutoff: DateTime<Utc>) -> Result<(), SubscriptionError> {
        Err(unused_store_call("advance_interval_anchor"))
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
