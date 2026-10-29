use crate::{
    csource_change_order::{CsourceChangeOrder, LatestWriteClaim},
    csource_processor::{CsourceNotificationProcessor, CsourceNotificationProcessorDeps},
    error::NotificationError,
    in_memory_ledger::InMemoryLedger,
};
use andromeda_context_commit::subscription_key::SubscriptionKey;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::{dispatch::TransportDispatcher, http::config::HttpTransportConfig, mqtt::config::MqttPoolConfig};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    csource_change_revision::CsourceChangeRevision,
    notification::{CsrChange, CsrChangeEvent},
    registration::ContextSourceRegistration,
    subscription::{Subscription, SubscriptionStatus},
};
use andromeda_ngsi_ld_query::{
    csr::{compiled::CompiledCsrQuery, expanded_entity_specification::ExpandedEntitySpecification},
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
use andromeda_subscription::{
    error::SubscriptionError,
    notification_slot::{NotificationOutcome, NotificationSlot, NotificationSlotGate, OutcomeRecord},
    store::{SubscriptionFields, SubscriptionStore},
    stored_subscription::StoredSubscription,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc};
use tokio::sync::broadcast::{self, error::TryRecvError};
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

const VEHICLE: &str = "https://example.com/Vehicle";
const SUBSCRIPTION: &str = "urn:ngsi-ld:Subscription:csource";
const REGISTRATION: &str = "urn:ngsi-ld:ContextSourceRegistration:1";

fn registration(id: &str, modified_at: &str) -> ContextSourceRegistration {
    serde_json::from_value(json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": VEHICLE}]}],
        "endpoint": "https://source.example.com/",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": modified_at
    }))
    .expect("registration parses")
}

fn instant(raw: &str) -> DateTime<Utc> {
    raw.parse().expect("valid instant")
}

/// The one change-driven subscription the tenant holds, notifying `endpoint`.
struct HeldSubscriptions {
    subscription: Subscription,
}

impl HeldSubscriptions {
    fn notifying(endpoint: &str) -> HeldSubscriptions {
        let subscription = serde_json::from_value::<Subscription>(json!({
            "id": SUBSCRIPTION,
            "type": "Subscription",
            "entities": [{"type": VEHICLE}],
            "notification": {"endpoint": {"uri": endpoint}},
            "status": "active",
            "createdAt": "2020-01-01T00:00:00Z",
            "modifiedAt": "2020-01-01T00:00:00Z"
        }))
        .expect("a valid subscription");
        HeldSubscriptions { subscription }
    }

    fn stored(&self) -> StoredSubscription {
        StoredSubscription {
            key: SubscriptionKey::try_from(1).expect("a positive key"),
            subscription: self.subscription.clone(), // clone: the port hands out owned subscriptions
        }
    }
}

#[async_trait]
impl SubscriptionStore for HeldSubscriptions {
    async fn get(&self, _target: &ContextTarget, id: &Identifier) -> Result<Option<StoredSubscription>, SubscriptionError> {
        Ok((self.subscription.id == *id).then(|| self.stored()))
    }

    async fn list(&self, _target: &ContextTarget, _window: &PageWindow) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
        Ok((vec![self.subscription.clone()], 1)) // clone: the port hands out owned subscriptions
    }

    async fn delete(&self, _target: &ContextTarget, _id: &Identifier) -> Result<u64, SubscriptionError> {
        Ok(0)
    }

    async fn exists(&self, _target: &ContextTarget, id: &Identifier) -> Result<bool, SubscriptionError> {
        Ok(self.subscription.id == *id)
    }

    async fn list_active(&self, _target: &ContextTarget) -> Result<Vec<StoredSubscription>, SubscriptionError> {
        Ok(vec![self.stored()])
    }

    async fn claim_notification_slot(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _gate: NotificationSlotGate,
    ) -> Result<NotificationSlot, SubscriptionError> {
        Ok(NotificationSlot::Claimed(OutcomeRecord::OutcomeOnly))
    }

    async fn record_outcome(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _outcome: NotificationOutcome,
        _record: OutcomeRecord,
    ) -> Result<(), SubscriptionError> {
        Ok(())
    }

    async fn expire_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<Identifier>, SubscriptionError> {
        Ok(Vec::new())
    }

    async fn find_due_interval_subscriptions(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<StoredSubscription>, SubscriptionError> {
        Ok(Vec::new())
    }

    async fn next_interval_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, SubscriptionError> {
        Ok(None)
    }

    async fn record_interval_tick(&self, _target: &ContextTarget, _id: &Identifier, _cutoff: DateTime<Utc>) -> Result<(), SubscriptionError> {
        Ok(())
    }

    async fn create_subscription(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _fields: &SubscriptionFields<'_>,
        _status: SubscriptionStatus,
    ) -> Result<(), SubscriptionError> {
        Ok(())
    }

    async fn update_subscription(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _fields: &SubscriptionFields<'_>,
        _status: SubscriptionStatus,
    ) -> Result<u64, SubscriptionError> {
        Ok(0)
    }
}

/// The registrations the tenant holds, as the store lists and reads them.
struct HeldRegistrations {
    held: Vec<ContextSourceRegistration>,
}

#[async_trait]
impl RegistrationStore for HeldRegistrations {
    async fn delete_expired(&self, _cutoff: DateTime<Utc>) -> Result<ExpiredDeletion, RegistrationError> {
        Ok(ExpiredDeletion {
            revision: CsourceChangeRevision::from(1),
            registrations: Vec::new(),
        })
    }

    async fn find_expired(&self, _cutoff: DateTime<Utc>) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        Ok(Vec::new())
    }

    async fn exists(&self, id: &Identifier) -> Result<bool, RegistrationError> {
        Ok(self.held.iter().any(|registration| registration.id == *id))
    }

    async fn create(&self, _id: &Identifier, _params: &CreateCsrParams) -> Result<WrittenRegistration, RegistrationError> {
        Err(RegistrationError::StoreUnavailable)
    }

    async fn get(&self, id: &Identifier) -> Result<Option<ContextSourceRegistration>, RegistrationError> {
        Ok(self.held.iter().find(|registration| registration.id == *id).cloned()) // clone: the port hands out owned registrations
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

    async fn find_matching(&self, _entities: &[ExpandedEntitySpecification<'_>]) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        Ok(Vec::new())
    }

    async fn find_overlapping(
        &self,
        _entity_id: Option<&Identifier>,
        _attributes: &[AttributeName],
    ) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        Ok(Vec::new())
    }

    async fn find_all_active(&self, after: Option<&Identifier>, _limit: u32) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        Ok(self
            .held
            .iter()
            .filter(|registration| after.is_none_or(|after| registration.id.as_str() > after.as_str()))
            .cloned() // clone: the port hands out owned registrations
            .collect())
    }

    async fn delete(&self, _id: &Identifier) -> Result<RegistrationMutation<CsourceChangeRevision>, RegistrationError> {
        Ok(RegistrationMutation::Missing)
    }

    async fn record_forwarding_outcome(&self, _id: &Identifier, _outcome: ForwardingOutcome, _at: DateTime<Utc>) -> Result<(), RegistrationError> {
        Ok(())
    }
}

/// A tenant whose subscription was written before every registration change.
struct SettledOrder;

impl CsourceChangeOrder for SettledOrder {
    fn written_after(&self, _revision: CsourceChangeRevision) -> BoxFuture<'_, Result<HashSet<Identifier>, NotificationError>> {
        Box::pin(async { Ok(HashSet::new()) })
    }

    fn claim_latest_write<'a>(&'a self, _subscription: &'a Identifier) -> BoxFuture<'a, Result<LatestWriteClaim, NotificationError>> {
        Box::pin(async { Ok(LatestWriteClaim::Settled) })
    }

    fn latest_write<'a>(&'a self, _subscription: &'a Identifier) -> BoxFuture<'a, Result<Option<CsourceChangeRevision>, NotificationError>> {
        Box::pin(async { Ok(None) })
    }
}

/// A tenant with one change-driven subscription notifying a live endpoint.
struct Tenant {
    processor: CsourceNotificationProcessor,
    ledger: Arc<InMemoryLedger>,
    subscriber: MockServer,
}

impl Tenant {
    async fn holding(registrations: Vec<ContextSourceRegistration>) -> Tenant {
        let subscriber = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(204)).mount(&subscriber).await;
        let ledger = Arc::new(InMemoryLedger::default());
        let processor = CsourceNotificationProcessor::new(CsourceNotificationProcessorDeps {
            target: ContextTarget::Live,
            sub_repo: Arc::new(HeldSubscriptions::notifying(&format!("{}/subscriber", subscriber.uri()))),
            csr_repo: Arc::new(HeldRegistrations { held: registrations }),
            notified: Arc::clone(&ledger) as Arc<_>,
            order: Arc::new(SettledOrder),
            engine: JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds"),
            transport: Arc::new(TransportDispatcher::new(HttpTransportConfig::default(), MqttPoolConfig::default()).expect("the dispatcher builds")),
            tenant: TenantRef::UnnamedDefault,
        });
        Tenant { processor, ledger, subscriber }
    }

    /// The trigger reason and registration identifiers of every notification
    /// the subscriber received, in arrival order.
    async fn received(&self) -> Vec<(String, Vec<String>)> {
        self.subscriber
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .map(|request| {
                let body: Value = serde_json::from_slice(&request.body).expect("the notification is JSON");
                let reason = body["triggerReason"].as_str().unwrap_or_default().to_owned();
                let carried = body["data"]
                    .as_array()
                    .map(|data| data.iter().filter_map(|member| member["id"].as_str()).map(str::to_owned).collect())
                    .unwrap_or_default();
                (reason, carried)
            })
            .collect()
    }
}

/// Overflow a channel of `capacity` with `sent` creations of the registration
/// and return the error its receiver then reports.
fn overflow(capacity: usize, sent: usize, written: &ContextSourceRegistration) -> TryRecvError {
    let (sender, mut receiver) = broadcast::channel(capacity);
    for _ in 0..sent {
        sender
            .send(CsrChangeEvent {
                csr_id: written.id.clone(), // clone: each event owns the identifier it reports
                change: CsrChange::Created {
                    written: Box::new(written.clone()), // clone: each event owns the registration it wrote
                },
                revision: CsourceChangeRevision::from(1),
            })
            .expect("a receiver is subscribed");
    }
    receiver.try_recv().expect_err("the receiver lagged")
}

/// Clause 5.11.2.4: a registration created while its change was lost to
/// channel overflow still reaches the subscriber, once.
#[tokio::test]
async fn a_registration_created_in_a_lagged_channel_still_reaches_the_subscriber() {
    let created = registration(REGISTRATION, "2024-01-02T00:00:00Z");
    let tenant = Tenant::holding(vec![created.clone()]).await; // clone: the channel and the store both hold it

    assert!(matches!(overflow(2, 5, &created), TryRecvError::Lagged(3)));
    tenant.processor.reconcile_registrations(None).await.expect("reconciled");
    tenant.processor.reconcile_registrations(None).await.expect("reconciled again");

    assert_eq!(tenant.received().await, vec![("newlyMatching".to_owned(), vec![REGISTRATION.to_owned()])]);
    assert_eq!(tenant.ledger.pairs().len(), 1);
}

/// Clause 5.3.3: a registration the subscriber was told of, modified while its
/// change was lost, is reported as updated.
#[tokio::test]
async fn a_registration_modified_in_a_lagged_channel_is_reported_as_updated() {
    let modified = registration(REGISTRATION, "2024-01-03T00:00:00Z");
    let tenant = Tenant::holding(vec![modified]).await;
    tenant.processor.reconcile_registrations(None).await.expect("initial reconciliation");

    let through = tenant
        .processor
        .reconcile_registrations(Some(instant("2024-01-02T00:00:00Z")))
        .await
        .expect("reconciled");

    assert_eq!(
        tenant.received().await,
        vec![
            ("newlyMatching".to_owned(), vec![REGISTRATION.to_owned()]),
            ("updated".to_owned(), vec![REGISTRATION.to_owned()]),
        ]
    );
    assert_eq!(through, Some(instant("2024-01-03T00:00:00Z")));
}
