//! Notifying one subscription of one committed entity change (clause 5.8.6).
//!
//! Every subscription a change reaches is notified the same way: the entity
//! document is stamped with the system attributes the subscription asked for,
//! built into the clause 5.3.1 notification, addressed to the subscription's
//! endpoint, and the outcome recorded on it. The concurrent fan-out over the
//! matched subscriptions therefore has a single body to run.

use crate::processor::{
    NotificationProcessor,
    compiled_transport,
    payload::{NotificationBuildParams, build_notification_data, cache::NotificationCompactionCache},
};
use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_context_space::{context_target::ContextTarget, snapshot_target::SnapshotTarget};
use andromeda_delivery::{
    dispatch::DeliveryResult,
    notification_context::NotificationContext,
    request::{NotificationDelivery, notification_media_type},
};
use andromeda_entity::sys_attrs::inject_sys_attrs;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::NotificationId};
use andromeda_ngsi_ld_model::notification::{Notification, NotificationType, SysAttrsSnapshot};
use andromeda_ngsi_ld_render::{geometry_selection::GeometrySelection, notification_data::notification_data};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_subscription::{
    compiled::CompiledSubscription,
    notification_slot::{NotificationSlot, NotificationSlotGate},
};
use chrono::Utc;
use serde_json::Value;
use tracing::{debug, error, warn};

/// The committed change every subscription matched by it is notified of.
///
/// One change produces one notification per matched subscription, and all of
/// them read the same branch, the same event and the same resolved system
/// attributes, so those are borrowed rather than copied per notification.
pub(super) struct NotifiedChange<'a> {
    /// The branch the change was committed on.
    pub(super) target: &'a ContextTarget,
    /// The committed change itself.
    pub(super) event: &'a ChangeEvent,
    /// The entity's clause 4.8 system attributes, resolved once for every
    /// subscription that asked for them.
    pub(super) sys_attrs: Option<&'a SysAttrsSnapshot>,
    /// Compaction and linked-entity memoization shared by the notifications
    /// this change produces.
    pub(super) compaction_cache: &'a NotificationCompactionCache,
}

/// The clause 5.3.1 notification document sent to `subscription`, or `None`
/// when it can be neither identified, shaped nor serialized.
///
/// Table 5.3.1-1: an endpoint whose `accept` is `application/geo+json`
/// receives the entities as one `FeatureCollection`.
fn notification_payload(subscription: &CompiledSubscription, entities: Vec<Value>) -> Option<Value> {
    let subscription_id = &subscription.id;
    let Ok(notification_id) = NotificationId::mint().map(Identifier::from) else {
        error!("Failed to construct notification URN for {}", subscription_id);
        return None;
    };
    let data = match notification_data(
        entities,
        notification_media_type(subscription.endpoint_accept.as_ref()),
        GeometrySelection::new(None, subscription.dataset_id.as_deref().unwrap_or_default()),
    ) {
        Ok(data) => data,
        Err(error) => {
            error!("Failed to render the notification data for {}: {}", subscription_id, error);
            return None;
        }
    };
    let notification = Notification::builder()
        .id(notification_id)
        .type_(NotificationType)
        // clone: the builder takes the identifier by value and the delivery still names it
        .subscription_id(subscription_id.clone())
        .notified_at(Utc::now())
        .data(data)
        .build();
    match serde_json::to_value(&notification) {
        Ok(payload) => Some(payload),
        Err(error) => {
            error!("Failed to serialize notification for {}: {}", subscription_id, error);
            None
        }
    }
}

impl NotificationProcessor {
    /// Notify one subscription of `change` and record the outcome on it
    /// (clause 5.8.6).
    ///
    /// `entity_data` is this notification's own copy of the entity document,
    /// because a subscription asking for system attributes has them injected
    /// into it. `None` is answered when the slot was refused, so no endpoint
    /// was addressed at all: either the subscription's clause 5.2.12
    /// `throttling` window or its endpoint's Table 5.2.15-1 `cooldown` window
    /// swallowed the notification.
    pub(super) async fn notify_subscription(
        &self,
        compiled_sub: &CompiledSubscription,
        change: &NotifiedChange<'_>,
        mut entity_data: Option<Value>,
    ) -> Option<DeliveryResult> {
        let subscription_id = &compiled_sub.id;
        match self
            .sub_repo
            .claim_notification_slot(
                change.target,
                subscription_id,
                NotificationSlotGate::at(Utc::now(), compiled_sub.throttling_secs, compiled_sub.endpoint_cooldown),
            )
            .await
        {
            Ok(NotificationSlot::Claimed) => {}
            Ok(NotificationSlot::Suppressed) => return None,
            Err(error) => {
                debug!("Failed to claim notification slot for {}: {}", subscription_id, error);
                return Some(DeliveryResult::RetryableFailure);
            }
        }

        if compiled_sub.notification_sys_attrs == SysAttrsInclusion::Include
            && let (Some(document), Some(sys_attrs)) = (entity_data.as_mut(), change.sys_attrs)
        {
            inject_sys_attrs(document, sys_attrs.created_at, sys_attrs.modified_at, sys_attrs.expires_at);
        }

        let Some(entity_data) = entity_data.as_ref() else {
            return Some(DeliveryResult::TerminalFailure);
        };
        let event = change.event;
        let build = NotificationBuildParams {
            target: change.target,
            engine: &self.engine,
            entity_data,
            previous_attributes: event.previous_attributes.as_deref(),
            sub: compiled_sub,
            change: &event.change,
            occurred_at: event.occurred_at,
            entity_query: &self.entity_query,
            compaction_cache: Some(change.compaction_cache),
        };
        let data = build_notification_data(&build).await;
        if data.is_empty() {
            debug!(
                "Subscription {} produced empty notification data for entity {}",
                subscription_id, event.entity_id
            );
            return Some(DeliveryResult::TerminalFailure);
        }

        let Some(payload) = notification_payload(compiled_sub, data) else {
            return Some(DeliveryResult::TerminalFailure);
        };
        let Some(endpoint_transport) = compiled_transport(compiled_sub) else {
            return Some(DeliveryResult::TerminalFailure);
        };
        // Clause 5.5.6: a `jsonldContext` that cannot be loaded names no context
        // this notification can be rendered under; the document may be reachable
        // again on a later attempt.
        let context = match NotificationContext::resolve(compiled_sub.jsonld_context.as_ref(), &self.engine).await {
            Ok(context) => context,
            Err(error) => {
                warn!("the jsonldContext of subscription {} cannot be loaded: {}", subscription_id, error);
                return Some(DeliveryResult::RetryableFailure);
            }
        };
        let request = NotificationDelivery::builder()
            .payload(&payload)
            .endpoint_uri(&compiled_sub.endpoint_uri)
            .accept(compiled_sub.endpoint_accept.as_ref())
            .receiver_info(compiled_sub.endpoint_receiver_info.as_deref())
            .context(&context)
            .tenant(self.tenant.tenant_name())
            .snapshot(change.target.snapshot().map(SnapshotTarget::snapshot_id))
            .timeout(compiled_sub.endpoint_timeout)
            .subscription_id(subscription_id)
            .build()
            .assemble();
        let delivery = self.transport.deliver(&endpoint_transport, &request).await;

        if delivery.is_delivered() {
            if let Err(error) = self.sub_repo.mark_success(change.target, subscription_id).await {
                debug!("Failed to mark notification success for {}: {}", subscription_id, error);
            }
        } else if let Err(error) = self.sub_repo.mark_failure(change.target, subscription_id).await {
            debug!("Failed to mark notification failure for {}: {}", subscription_id, error);
        }
        Some(delivery)
    }
}
