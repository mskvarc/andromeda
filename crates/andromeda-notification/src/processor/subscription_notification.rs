//! Notifying one subscription of one committed entity change (clause 5.8.6).
//!
//! Every subscription a change reaches is notified the same way: the entity
//! state is rendered as the subscription asks, built into the clause 5.3.1
//! notification, addressed to the subscription's endpoint, and the outcome
//! recorded on it. The concurrent fan-out over the matched subscriptions
//! therefore has a single body to run.

use crate::{
    notification_outcome::notification_outcome,
    processor::{
        NotificationProcessor,
        attempt_failure::NotificationAttemptFailure,
        compiled_transport,
        notification_identity::NotificationIdentity,
        payload::{NotificationBuildParams, build_notification_data, cache::NotificationCompactionCache},
    },
    resolved_notification_context::{UnclaimedContext, unclaimed_notification_context},
};
use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_context_space::{context_target::ContextTarget, snapshot_target::SnapshotTarget};
use andromeda_delivery::{dispatch::DeliveryResult, request::NotificationDelivery};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_render::{geometry_selection::GeometrySelection, notification_data::DataShape};
use andromeda_subscription::{
    compiled::CompiledSubscription,
    notification_slot::{NotificationSlot, NotificationSlotGate},
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tracing::{debug, error};

/// The committed change every subscription matched by it is notified of.
///
/// One change produces one notification per matched subscription, and all of
/// them read the same branch, the same event, the same entity state and the
/// same resolved system attributes, so those are borrowed rather than copied
/// per notification.
pub(super) struct NotifiedChange<'a> {
    /// The branch the change was committed on.
    pub(super) target: &'a ContextTarget,
    /// The committed change itself.
    pub(super) event: &'a ChangeEvent,
    /// The entity the change is notified with, `None` when there is nothing
    /// left to notify about.
    pub(super) notified: Option<&'a Arc<EntityState>>,
    /// The instant every notification of the change reads the entity at.
    pub(super) now: DateTime<Utc>,
    /// Compaction and linked-entity memoization shared by the notifications
    /// this change produces.
    pub(super) compaction_cache: &'a NotificationCompactionCache,
    /// Which identifier the notifications of the change carry.
    pub(super) identity: NotificationIdentity,
}

impl NotificationProcessor {
    /// Notify one subscription of `change` and record the outcome on it
    /// (clause 5.8.6).
    ///
    /// `None` is answered when no endpoint was addressed at all: the
    /// subscription expired before the notification was generated, or the
    /// slot was refused because the subscription's clause 5.2.12 `throttling`
    /// window or its endpoint's Table 5.2.15-1 `cooldown` window swallowed the
    /// notification.
    pub(super) async fn notify_subscription(
        &self,
        compiled_sub: &CompiledSubscription,
        change: &NotifiedChange<'_>,
    ) -> Option<Result<(), NotificationAttemptFailure>> {
        let subscription_id = &compiled_sub.id;
        // Clause 5.8.1.4: no notification is sent once `expiresAt` is reached.
        // The match judged the subscription in force when the change was
        // evaluated, so the send is admitted again at the instant its
        // `notifiedAt` carries.
        let generated_at = Utc::now();
        if !compiled_sub.standing.notifies_at(generated_at) {
            return None;
        }
        let gate = NotificationSlotGate::at(generated_at, compiled_sub.cadence(), compiled_sub.endpoint_cooldown);
        // Clause 5.5.6: a `jsonldContext` that cannot be loaded names no context
        // this notification can be rendered under. It is resolved before the
        // slot is claimed so that failure takes no throttling slot.
        let context = match unclaimed_notification_context(
            &self.engine,
            compiled_sub.jsonld_context.as_ref(),
            self.sub_repo.as_ref(),
            change.target,
            subscription_id,
            gate,
        )
        .await
        {
            UnclaimedContext::Resolved(context) => context,
            UnclaimedContext::Failed => return Some(Err(NotificationAttemptFailure::ContextUnloadable)),
            UnclaimedContext::Suppressed => return None,
        };
        let record = match self.sub_repo.claim_notification_slot(change.target, subscription_id, gate).await {
            Ok(NotificationSlot::Claimed(record)) => record,
            Ok(NotificationSlot::Suppressed) => return None,
            Err(error) => {
                debug!("Failed to claim notification slot for {}: {}", subscription_id, error);
                return Some(Err(NotificationAttemptFailure::SlotUnclaimed(error)));
            }
        };

        let Some(notified) = change.notified else {
            return Some(Err(NotificationAttemptFailure::NothingToNotify));
        };
        let endpoint_transport = match compiled_transport(compiled_sub) {
            Ok(transport) => transport,
            Err(error) => return Some(Err(NotificationAttemptFailure::EndpointUndeliverable(error))),
        };
        let delivery = NotificationDelivery::builder()
            .endpoint_uri(&compiled_sub.endpoint_uri)
            .accept(compiled_sub.endpoint_accept)
            .receiver_info(compiled_sub.endpoint_receiver_info.as_deref())
            .context(&context)
            .tenant(self.tenant.tenant_name())
            .snapshot(change.target.snapshot().map(SnapshotTarget::snapshot_id))
            .timeout(compiled_sub.endpoint_timeout)
            .subscription_id(subscription_id)
            .build();
        let event = change.event;
        let build = NotificationBuildParams {
            target: change.target,
            engine: &self.engine,
            notified,
            previous: event.previous_attributes.as_deref(),
            sys_attrs: event.sys_attrs_snapshot.as_ref(),
            sub: compiled_sub,
            change: &event.change,
            occurred_at: event.occurred_at,
            now: change.now,
            entity_query: &self.entity_query,
            cache: change.compaction_cache,
        };
        // Table 5.3.1-1: an endpoint whose `accept` is `application/geo+json`
        // receives the entities as one `FeatureCollection`.
        let shape = DataShape {
            media_type: delivery.content_type(),
            selection: GeometrySelection::new(None, compiled_sub.dataset_id.as_deref().unwrap_or_default()),
            context: delivery.body_context(),
        };
        let Some(data) = build_notification_data(&build, shape).await else {
            debug!("Subscription {} produced no notification data for entity {}", subscription_id, event.entity_id);
            return Some(Err(NotificationAttemptFailure::NoData));
        };
        let request = match change.identity.notification(&delivery, subscription_id, data, generated_at) {
            Ok(request) => request,
            Err(error) => {
                error!("Failed to write the notification for {}: {}", subscription_id, error);
                return Some(Err(NotificationAttemptFailure::Unwritten(error)));
            }
        };
        let delivery = self.transport.deliver(&endpoint_transport, &request).await;

        if let Err(error) = self
            .outcomes
            .record(change.target, subscription_id, notification_outcome(&delivery), record)
            .await
        {
            debug!("Failed to record the notification outcome for {}: {}", subscription_id, error);
        }
        Some(match delivery {
            DeliveryResult::Delivered => Ok(()),
            DeliveryResult::Failed(failure) => Err(NotificationAttemptFailure::Endpoint(failure)),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        notifier_test_double::{ClaimRecordingStore, NoEntities, entity_query, offline_engine, transport},
        pending_change::PendingCommits,
        processor::{
            NotificationProcessor,
            NotificationProcessorDeps,
            notification_identity::NotificationIdentity,
            payload::cache::NotificationCompactionCache,
            subscription_notification::NotifiedChange,
        },
    };
    use andromeda_context_commit::{change_event::ChangeEvent, store_instant::StoreInstant, subscription_key::SubscriptionKey};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::rendering::instant_text};
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::ChangeOrigin,
    };
    use andromeda_ngsi_ld_query::filter::language_matching::LanguageMatching;
    use andromeda_subscription::{
        cache::SubscriptionCache,
        compilation::compile_subscription,
        compiled::CompiledSubscription,
        notifying_since::NotifyingSince,
        stored_subscription::StoredSubscription,
    };
    use andromeda_tenancy::tenant_ref::TenantRef;
    use chrono::{DateTime, TimeDelta, Utc};
    use serde_json::json;
    use std::sync::Arc;

    fn processor(store: &Arc<ClaimRecordingStore>) -> NotificationProcessor {
        let entities = Arc::new(NoEntities);
        NotificationProcessor::new(NotificationProcessorDeps {
            live_target: ContextTarget::Live,
            sub_repo: Arc::<ClaimRecordingStore>::clone(store),
            entity_reader: Arc::<NoEntities>::clone(&entities),
            entity_query: entity_query(&entities),
            engine: offline_engine(),
            transport: transport(),
            tenant: TenantRef::UnnamedDefault,
            subscription_cache: Arc::new(SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality)),
            pending_commits: Arc::new(PendingCommits::default()),
        })
    }

    /// A change-driven subscription on every Vehicle that expires at
    /// `expires_at`.
    fn expiring_at(expires_at: DateTime<Utc>) -> CompiledSubscription {
        let subscription = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Subscription:expiring",
            "type": "Subscription",
            "entities": [{"type": "https://example.org/Vehicle"}],
            "expiresAt": instant_text(expires_at),
            "notification": {"endpoint": {"uri": "http://subscriber.example.org/notify", "accept": "application/json"}},
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid subscription");
        compile_subscription(
            &StoredSubscription {
                key: SubscriptionKey::try_from(1).expect("a valid key"),
                subscription,
                notifying_since: NotifyingSince {
                    active: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
                    selected: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
                },
            },
            LanguageMatching::TagEquality,
        )
    }

    fn vehicle() -> Arc<EntityState> {
        let document = json!([{"@id": "urn:ngsi-ld:Vehicle:1", "@type": ["https://example.org/Vehicle"]}]);
        Arc::new(EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity decodes"))
    }

    /// Notify `subscription` of the creation of a Vehicle evaluated at
    /// `evaluated_at`, answering the subscriptions whose slot was asked for.
    async fn notify(subscription: &CompiledSubscription, evaluated_at: DateTime<Utc>) -> Vec<Identifier> {
        let store = Arc::new(ClaimRecordingStore::default());
        let notified = vehicle();
        let event = ChangeEvent::builder()
            .entity_id(notified.id().clone()) // clone: the event owns the identifier it carries
            .entity_types(notified.types().as_slice().to_vec())
            .change(EntityChange::new(EntityLifecycle::Created, Vec::new()))
            .occurred_at(evaluated_at)
            .origin(ChangeOrigin::Local)
            .build();
        let compaction_cache = NotificationCompactionCache::default();
        let change = NotifiedChange {
            target: &ContextTarget::Live,
            event: &event,
            notified: Some(&notified),
            now: evaluated_at,
            compaction_cache: &compaction_cache,
            identity: NotificationIdentity::Live,
        };
        let sent = processor(&store).notify_subscription(subscription, &change).await;
        assert!(sent.is_none(), "the store refuses every slot, so no endpoint is addressed");
        store.claims().into_iter().map(|(id, _gate)| id).collect()
    }

    /// Clause 5.8.1.4: no notification is sent once `expiresAt` is reached.
    /// The change was evaluated while the subscription was in force, but the
    /// notification would be generated after its expiry, so no slot is claimed.
    #[tokio::test]
    async fn a_change_evaluated_before_expires_at_and_generated_after_it_sends_nothing() {
        let expires_at = Utc::now() - TimeDelta::seconds(1);

        let claimed = notify(&expiring_at(expires_at), expires_at - TimeDelta::seconds(1)).await;

        pretty_assertions::assert_eq!(claimed, Vec::<Identifier>::new());
    }

    /// A notification generated before `expiresAt` claims its slot.
    #[tokio::test]
    async fn a_change_generated_before_expires_at_claims_its_slot() {
        let subscription = expiring_at(Utc::now() + TimeDelta::hours(1));

        let claimed = notify(&subscription, Utc::now()).await;

        pretty_assertions::assert_eq!(claimed, vec![subscription.id]);
    }
}
