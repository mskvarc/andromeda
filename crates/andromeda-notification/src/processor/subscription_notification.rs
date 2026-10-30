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
        compiled_transport,
        payload::{NotificationBuildParams, build_notification_data, cache::NotificationCompactionCache},
    },
    resolved_notification_context::{UnclaimedContext, unclaimed_notification_context},
};
use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_context_space::{context_target::ContextTarget, snapshot_target::SnapshotTarget};
use andromeda_delivery::{dispatch::DeliveryResult, request::NotificationDelivery};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
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
    /// The entity's clause 4.8 system attributes, resolved once for every
    /// subscription that asked for them.
    pub(super) sys_attrs: Option<&'a SysAttrsSnapshot>,
    /// Compaction and linked-entity memoization shared by the notifications
    /// this change produces.
    pub(super) compaction_cache: &'a NotificationCompactionCache,
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
    pub(super) async fn notify_subscription(&self, compiled_sub: &CompiledSubscription, change: &NotifiedChange<'_>) -> Option<DeliveryResult> {
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
            UnclaimedContext::Failed => return Some(DeliveryResult::RetryableFailure),
            UnclaimedContext::Suppressed => return None,
        };
        let record = match self.sub_repo.claim_notification_slot(change.target, subscription_id, gate).await {
            Ok(NotificationSlot::Claimed(record)) => record,
            Ok(NotificationSlot::Suppressed) => return None,
            Err(error) => {
                debug!("Failed to claim notification slot for {}: {}", subscription_id, error);
                return Some(DeliveryResult::RetryableFailure);
            }
        };

        let Some(notified) = change.notified else {
            return Some(DeliveryResult::TerminalFailure);
        };
        let Some(endpoint_transport) = compiled_transport(compiled_sub) else {
            return Some(DeliveryResult::TerminalFailure);
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
            sys_attrs: change.sys_attrs,
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
            return Some(DeliveryResult::TerminalFailure);
        };
        let request = match delivery.notification(data, generated_at) {
            Ok(request) => request,
            Err(error) => {
                error!("Failed to write the notification for {}: {}", subscription_id, error);
                return Some(DeliveryResult::TerminalFailure);
            }
        };
        let delivery = self.transport.deliver(&endpoint_transport, &request).await;

        if let Err(error) = self
            .sub_repo
            .record_outcome(change.target, subscription_id, notification_outcome(&delivery), record)
            .await
        {
            debug!("Failed to record the notification outcome for {}: {}", subscription_id, error);
        }
        Some(delivery)
    }
}
