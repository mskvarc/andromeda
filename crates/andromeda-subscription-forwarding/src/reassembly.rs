//! Re-assembling an inbound notification for the local subscriber (clause 5.8.6).
//!
//! A notification arriving under a forwarded copy's identifier is copied for
//! the local subscription. When entities may be split, each entity in its
//! `data` is completed from this broker's own copy and from every other
//! registered source that holds it — never the sender, which already supplied
//! its part — and the entities that fail the local subscription's filters are
//! dropped. Whatever remains is delivered to the local endpoint under the local
//! subscription's identifier.

use crate::{error::SubscriptionForwardingError, service::SubscriptionForwardingService};
use andromeda_delivery::{endpoint::transport::parse_endpoint_transport, request::NotificationDelivery};
use andromeda_entity::{error::EntityError, query::retrieval::RetrievalNarrowing};
use andromeda_federation::{
    entity_result::merge_retrieve_results,
    operation::ForwardingOperation,
    outcome::ForwardedEntities,
    request::{CsrModeFilter, ForwardRetrieveRequest, TriggeringRequest},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    uri::{NGSI_LD_URN_NID, Uri},
};
use andromeda_ngsi_ld_model::{
    entity::Entity,
    notification::{Notification, NotificationType},
    registration::ForwardingMode,
    subscription::Subscription,
};
use andromeda_ngsi_ld_render::entity::EntityRenderer;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_subscription::{compilation::compile_subscription, matching::matches_entity_filters};
use chrono::Utc;
use serde_json::Value;
use tracing::{debug, warn};
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// The members of an entity document that are never attributes.
const STRUCTURAL_MEMBERS: [&str; 7] = ["@context", "id", "type", "scope", "createdAt", "modifiedAt", "expiresAt"];

/// Narrow a rendered entity to the attributes the local subscription delivers
/// (clause 5.8.6 `notification.attributes`, `pick` and `omit`).
fn narrow_rendered(mut rendered: Value, subscription: &Subscription) -> Value {
    let Some(object) = rendered.as_object_mut() else {
        return rendered;
    };
    let keep_only = |names: &[AttributeName], object: &mut serde_json::Map<String, Value>| {
        object.retain(|key, _| STRUCTURAL_MEMBERS.contains(&key.as_str()) || names.iter().any(|name| name.as_str() == key));
    };
    if let Some(attributes) = subscription.notification.attributes.as_deref() {
        keep_only(attributes, object);
    }
    if let Some(pick) = subscription.notification.pick.as_deref() {
        keep_only(pick, object);
    }
    if let Some(omit) = subscription.notification.omit.as_deref() {
        object.retain(|key, _| !omit.iter().any(|name| name.as_str() == key));
    }
    rendered
}

/// Mint a notification identifier.
fn notification_id() -> Result<Uri, SubscriptionForwardingError> {
    let urn = UrnBuilder::new(NGSI_LD_URN_NID, &format!("Notification:{}", Uuid::new_v4()))
        .build()
        .map_err(|error| SubscriptionForwardingError::internal(error.to_string()))?;
    Ok(Uri::from(urn))
}

impl SubscriptionForwardingService {
    /// Complete one entity of the notification from this broker and from every
    /// other holder (clause 5.8.6 split entities).
    async fn reassemble_entity(&self, received: Entity, sender: &Uri, context: &ContextSpec) -> Result<Entity, SubscriptionForwardingError> {
        let narrowing = RetrievalNarrowing {
            attrs: &[],
            sys_attrs: SysAttrsInclusion::Strip,
            dataset_ids: &[],
            projection: None,
        };
        let local = match self.entity_query.get_inner(&received.id, &narrowing, context).await {
            Ok(entity) => Some(entity),
            Err(EntityError::NotFound { .. }) => None,
            Err(error) => return Err(error.into()),
        };
        let remote = self
            .federation
            .forward_retrieve(ForwardRetrieveRequest {
                entity_id: &received.id,
                exclude_registration: Some(sender),
                query_string: "",
                context,
                operation: ForwardingOperation::RetrieveEntity,
                triggering: TriggeringRequest::default(),
                mode_filter: CsrModeFilter::All,
                entity_map: None,
            })
            .await?;
        // The received part is the base; the local copy and the other holders
        // are folded in the way a distributed retrieval merges its sources.
        let mut sources = Vec::with_capacity(remote.len() + 1);
        if let Some(local) = local {
            sources.push(ForwardedEntities {
                registration_id: None,
                mode: ForwardingMode::Inclusive,
                entities: vec![local],
                warnings: Vec::new(),
            });
        }
        sources.extend(remote);
        merge_retrieve_results(Some(received), sources)
            .ok_or_else(|| SubscriptionForwardingError::internal("merging a present entity with its other parts produced nothing"))
    }

    /// Handle a notification arriving under a forwarded copy's identifier.
    pub async fn handle_notification(&self, notification: &Notification, context: &ContextSpec) -> Result<(), SubscriptionForwardingError> {
        let Some((local, sender)) = self.mappings.local_of_forwarded(&notification.subscription_id).await? else {
            return Err(SubscriptionForwardingError::UnknownSubscription {
                subscription: notification.subscription_id.clone(), // clone: the error owns the identifier
            });
        };
        let Some(stored) = self.subscriptions.get(&local).await? else {
            debug!(subscription = %local, "notification for a forwarded copy whose local subscription no longer exists");
            return Ok(());
        };
        let compiled = compile_subscription(&stored);
        let subscription = stored.subscription;
        let split = self.federation.split_entities(subscription.delivery.split_entities);

        let mut data = Vec::with_capacity(notification.data.len());
        for value in &notification.data {
            let received: Entity = match serde_json::from_value(value.clone()) {
                // clone: deserialisation consumes the value while the notification stays borrowed
                Ok(entity) => entity,
                Err(error) => {
                    warn!(subscription = %local, %error, "inbound notification carried an entity that does not parse");
                    continue;
                }
            };
            let entity = if split {
                self.reassemble_entity(received, &sender, context).await?
            } else {
                received
            };
            // Clause 5.8.6: entities failing the local filters leave the copy.
            let expanded = self.engine.expand(serde_json::to_value(&entity)?, context).await?;
            if !matches_entity_filters(&compiled, &expanded) {
                continue;
            }
            let rendered = EntityRenderer::new(&entity)
                .with_mode(subscription.notification.format.unwrap_or_default())
                .render()
                .map_err(|error| SubscriptionForwardingError::internal(error.to_string()))?;
            data.push(narrow_rendered(rendered, &subscription));
        }
        if data.is_empty() {
            debug!(subscription = %local, "re-assembled notification carries no entity; nothing delivered");
            return Ok(());
        }

        let copy = Notification {
            id: notification_id()?,
            type_: NotificationType,
            subscription_id: local.clone(), // clone: the copy names the local subscription
            notified_at: Utc::now(),
            data,
        };
        let payload = serde_json::to_value(&copy)?;
        let endpoint = &subscription.notification.endpoint;
        let transport = parse_endpoint_transport(&endpoint.uri, endpoint.notifier_info.as_deref())
            .map_err(|error| SubscriptionForwardingError::internal(format!("local subscription endpoint is not deliverable: {error}")))?;
        let request = NotificationDelivery {
            payload: &payload,
            endpoint_uri: &endpoint.uri,
            accept: endpoint.accept.as_ref(),
            receiver_info: endpoint.receiver_info.as_deref(),
            jsonld_context: subscription.delivery.jsonld_context.as_ref(),
            tenant: self.tenant.tenant_name(),
            subscription_id: &local,
        }
        .assemble();
        let result = self.transport.deliver(&transport, &request).await;
        if result.is_delivered() {
            self.subscriptions.mark_success(&local).await?;
        } else {
            self.subscriptions.mark_failure(&local).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::reassembly::narrow_rendered;
    use andromeda_ngsi_ld_model::subscription::Subscription;
    use serde_json::json;

    fn subscription(notification: serde_json::Value) -> Subscription {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Subscription:local",
            "type": "Subscription",
            "entities": [{"type": "Vehicle"}],
            "notification": notification,
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid subscription")
    }

    #[test]
    fn attributes_pick_and_omit_narrow_the_delivered_entity_but_never_its_structure() {
        let rendered =
            json!({"id": "urn:a", "type": "Vehicle", "speed": {"type": "Property", "value": 1}, "colour": {"type": "Property", "value": "red"}, "scope": "/x"});
        let attributes = subscription(json!({"attributes": ["speed"], "endpoint": {"uri": "http://s/n"}}));
        let narrowed = narrow_rendered(rendered.clone(), &attributes); // clone: reused below
        assert!(narrowed.get("speed").is_some() && narrowed.get("colour").is_none() && narrowed.get("scope").is_some());
        let omit = subscription(json!({"omit": ["speed"], "endpoint": {"uri": "http://s/n"}}));
        let narrowed = narrow_rendered(rendered, &omit);
        assert!(narrowed.get("speed").is_none() && narrowed.get("colour").is_some());
    }
}
