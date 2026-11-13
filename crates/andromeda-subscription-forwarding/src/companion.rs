//! The companion context source registration subscription of a local
//! subscription (clauses 5.8.1.4, 5.8.2.4 and 5.8.5.4).
//!
//! A local subscription that is not `localOnly` is mirrored by a registration
//! subscription over the same entities and attributes whose notifications reach
//! this broker's receiver; those notifications drive the reduced copies placed
//! at the matching context sources. An update follows the original, and a
//! deletion or an expiry tears down the companion together with every
//! forwarded copy.

use crate::{
    error::SubscriptionForwardingError,
    forwarder::{ForwardDecision, decide},
    minted_subscription_id::mint_subscription_id,
    participation::stays_local,
    receiver::ReceiverEndpoint,
    service::SubscriptionForwardingService,
    teardown::CopyMappings,
};
use andromeda_jsonld::user_context_admission::AdmittedContext;
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    clearable::Clearable,
    notification::CsrTriggerReason,
    notification_request::NotificationRequest,
    registration::ContextSourceRegistration,
    subscription::{Subscription, SubscriptionType},
    subscription_request::SubscriptionRequest,
};
use andromeda_subscription::{
    compilation::compile_csource_subscription,
    compiled::CompiledCsrSubscription,
    csource_matching::csource_subscription_selects_registration,
    deleted_members::DeletedMembers,
    error::SubscriptionError,
    forwarding_hook::{ForwardedCopy, SubscriptionForwardingHook, SubscriptionTeardown},
    subscription_context::SubscriptionContext,
    subscription_invariant::SubscriptionInvariant,
};
use chrono::Utc;
use futures::future::BoxFuture;
use tracing::debug;

/// The companion request mirroring `subscription` (clause 5.11.2): the same
/// entity selectors, watched attributes and notified attributes, matched
/// against registrations rather than entities, notifying this broker.
///
/// It names neither an identifier nor a type, so it is an update fragment as
/// it stands; a create adds both.
fn companion_request(subscription: &Subscription, receiver: &ReceiverEndpoint) -> SubscriptionRequest {
    SubscriptionRequest {
        entities: Clearable::from_option(subscription.selection.entities.clone()),
        watched_attributes: Clearable::from_option(subscription.selection.watched_attributes.clone()),
        notification: Some(NotificationRequest {
            attributes: subscription.notification.attributes.clone(),
            endpoint: Some(receiver.notification_endpoint()),
            ..NotificationRequest::default()
        }),
        ..SubscriptionRequest::default()
    }
}

/// Registrations read per round trip while the copies of a new subscription
/// are placed.
const REGISTRATION_PAGE: u32 = 200;

impl SubscriptionForwardingService {
    /// Create the companion of `subscription`, place the copies it already
    /// calls for, and only then route its identifier here.
    ///
    /// The companion's own "newlyMatching" notification is on its way the
    /// moment the companion exists. Routing it to this subscription only after
    /// every copy is placed and recorded keeps that notification from placing
    /// a second copy at a source whose first one is not yet recorded; one
    /// that arrives before the route exists is refused and carries nothing
    /// the synchronous placement has not already done.
    async fn create_companion(
        &self,
        subscription: &Subscription,
        encountered_sources: &[ContextSourcePseudonym],
    ) -> Result<Identifier, SubscriptionForwardingError> {
        let request = SubscriptionRequest {
            id: Some(mint_subscription_id()?),
            type_: Some(SubscriptionType),
            ..companion_request(subscription, &self.receiver)
        };
        // The companion is a local resource; its own creation passes through no
        // context source, so it carries no `Via` chain of its own.
        let companion = self
            .csource_subscriptions
            .create(&self.live_target, request, SubscriptionContext::CoreOnly, &[])
            .await?;
        self.place_initial_copies(subscription, &companion, encountered_sources).await?;
        self.mappings.link_companion(&subscription.id, &companion).await?;
        self.router.register(&companion, &self.tenant).await?;
        debug!(subscription = %subscription.id, %companion, "companion context source registration subscription created");
        Ok(companion)
    }

    /// The companion as the matcher its notifications are decided by.
    async fn compiled_companion(&self, companion: &Identifier) -> Result<CompiledCsrSubscription, SubscriptionForwardingError> {
        Ok(compile_csource_subscription(
            &self.csource_subscriptions.get(&self.live_target, companion).await?,
        ))
    }

    /// Place a reduced copy at every registered source the companion already
    /// selects (clauses 5.11.2.4 and 5.12), before the creation answers.
    ///
    /// The companion's own "newlyMatching" notification reports the same
    /// sources, but it is delivered after the creation has been answered, and
    /// a change a source sees in between would reach no copy. A source the
    /// notification names again already holds its copy and is left alone.
    async fn place_initial_copies(
        &self,
        subscription: &Subscription,
        companion: &Identifier,
        encountered_sources: &[ContextSourcePseudonym],
    ) -> Result<(), SubscriptionForwardingError> {
        let compiled = self.compiled_companion(companion).await?;
        let store = self.federation.registration_store();
        let mut after: Option<Identifier> = None;
        loop {
            let page = store
                .find_all_active(after.as_ref(), REGISTRATION_PAGE)
                .await
                .map_err(SubscriptionForwardingError::from)?;
            let exhausted = page.len() < REGISTRATION_PAGE as usize;
            for registration in page {
                after = Some(registration.id.clone()); // clone: the cursor outlives the page it was read from
                // Table 6.3.18-2 / clause 5.12: never forward back to a context
                // source the creating request already passed through.
                if registration
                    .access
                    .context_source_alias
                    .as_ref()
                    .is_some_and(|alias| encountered_sources.contains(alias))
                {
                    continue;
                }
                if !csource_subscription_selects_registration(&compiled, &registration) {
                    continue;
                }
                let decision = decide(CsrTriggerReason::NewlyMatching, registration.access.mode.unwrap_or_default(), None);
                self.apply_forward_decision(subscription, &registration, decision, &DeletedMembers::default())
                    .await?;
            }
            if exhausted {
                return Ok(());
            }
        }
    }

    /// What an update of `subscription` calls for at a source already holding
    /// a copy: an update while the registration still matches, a deletion once
    /// it no longer does (clause 5.8.1.4 "updated" / "noLongerMatching").
    fn decision_after_update(compiled: &CompiledCsrSubscription, registration: &ContextSourceRegistration, copy: &ForwardedCopy) -> ForwardDecision {
        let trigger = if csource_subscription_selects_registration(compiled, registration) {
            CsrTriggerReason::Updated
        } else {
            CsrTriggerReason::NoLongerMatching
        };
        decide(trigger, registration.access.mode.unwrap_or_default(), Some(copy.remote_subscription.clone()))
    }

    /// Follow an update of `subscription` on its companion and on every
    /// forwarded copy (clause 5.8.2.4).
    ///
    /// An update that leaves the subscription expired withdraws it as the
    /// expiry did, and one that makes it active again finds no companion and
    /// provisions one, which places the copies the update calls for.
    async fn follow_update(&self, subscription: &Subscription, deleted: &DeletedMembers) -> Result<(), SubscriptionForwardingError> {
        if stays_local(subscription, Utc::now()) {
            return self.withdraw(&subscription.id).await;
        }
        let Some(companion) = self.update_companion(subscription).await? else {
            // An update that first provisions the companion carries no
            // creation-time `Via` chain here; the update path forwards to
            // every selected source.
            self.create_companion(subscription, &[]).await?;
            return Ok(());
        };
        let compiled = self.compiled_companion(&companion).await?;
        for copy in self.mappings.forwarded_of(&subscription.id).await? {
            if let Some(registration) = self.federation_registration(&copy.registration).await? {
                let decision = Self::decision_after_update(&compiled, &registration, &copy);
                self.apply_forward_decision(subscription, &registration, decision, deleted).await?;
            }
        }
        Ok(())
    }

    /// Update the companion of `subscription` to mirror it, answering the
    /// companion updated, or `None` when no companion stands.
    ///
    /// A companion the mappings name but the store no longer holds was deleted
    /// underneath its mapping. Its route and every copy still recorded are
    /// withdrawn, so the companion provisioned in its place starts from no
    /// copy; linking that companion replaces the stale mapping.
    async fn update_companion(&self, subscription: &Subscription) -> Result<Option<Identifier>, SubscriptionForwardingError> {
        let Some(companion) = self.mappings.companion_of(&subscription.id).await? else {
            return Ok(None);
        };
        let updated = self
            .csource_subscriptions
            .update(
                &self.live_target,
                &companion,
                companion_request(subscription, &self.receiver),
                &AdmittedContext::core_only(),
            )
            .await;
        match updated {
            Ok(()) => Ok(Some(companion)),
            Err(SubscriptionError::NotFound { .. }) => {
                debug!(subscription = %subscription.id, %companion, "companion subscription is gone; provisioning a new one");
                let leftover = SubscriptionTeardown {
                    companion: None,
                    forwarded: self.mappings.forwarded_of(&subscription.id).await?,
                };
                self.tear_down(&subscription.id, &leftover, CopyMappings::StillRecorded).await;
                self.router.unregister(&companion).await?;
                Ok(None)
            }
            Err(other) => Err(other.into()),
        }
    }

    /// Follow the expiry of `local` (clause 5.8.1.4) by withdrawing its
    /// companion and every forwarded copy, unless a step that ran between the
    /// committed expiry and this one changed the subscription: an update that
    /// made it active again keeps what it placed, and a deletion has torn
    /// everything down already.
    async fn follow_expiry(&self, local: &Identifier) -> Result<(), SubscriptionForwardingError> {
        let Some(stored) = self.subscriptions.get(&self.live_target, local).await? else {
            debug!(subscription = %local, "expired subscription was deleted before its withdrawal");
            return Ok(());
        };
        if !stays_local(&stored.subscription, Utc::now()) {
            debug!(subscription = %local, "expired subscription was made active again; nothing is withdrawn");
            return Ok(());
        }
        self.withdraw(local).await
    }
}

impl SubscriptionForwardingHook for SubscriptionForwardingService {
    fn created<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        subscription: &'life1 Subscription,
        encountered_sources: &'life2 [ContextSourcePseudonym],
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            if stays_local(subscription, Utc::now()) {
                return Ok(());
            }
            let _step = self.lifecycle_steps.async_lock(subscription.id.clone()).await;
            self.create_companion(subscription, encountered_sources)
                .await
                .map_err(into_subscription_error)?;
            Ok(())
        })
    }

    fn updated<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        subscription: &'life1 Subscription,
        deleted: &'life2 DeletedMembers,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let _step = self.lifecycle_steps.async_lock(subscription.id.clone()).await;
            // The update future is large, so it is boxed rather than held inline.
            Box::pin(self.follow_update(subscription, deleted)).await.map_err(into_subscription_error)
        })
    }

    fn teardown_of<'life0, 'life1, 'async_trait>(
        &'life0 self,
        id: &'life1 Identifier,
    ) -> BoxFuture<'async_trait, Result<SubscriptionTeardown, SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move { self.teardown_of(id).await.map_err(into_subscription_error) })
    }

    fn deleted<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        id: &'life1 Identifier,
        teardown: &'life2 SubscriptionTeardown,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let _step = self.lifecycle_steps.async_lock(id.clone()).await;
            self.tear_down(id, teardown, CopyMappings::RemovedWithSubscription).await;
            Ok(())
        })
    }

    fn expired<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Identifier) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let _step = self.lifecycle_steps.async_lock(id.clone()).await;
            self.follow_expiry(id).await.map_err(into_subscription_error)
        })
    }
}

/// Report a distributed-side failure through the hook's error type.
fn into_subscription_error(error: SubscriptionForwardingError) -> SubscriptionError {
    match error {
        SubscriptionForwardingError::Subscription(inner) => inner,
        // Saturation stays saturation across the boundary so it is still shed as
        // a 503 rather than folded into a persistence fault.
        SubscriptionForwardingError::StoreSaturated => SubscriptionError::StoreSaturated,
        SubscriptionForwardingError::StoreUnavailable => SubscriptionError::StoreUnavailable,
        SubscriptionForwardingError::Persistence(failure) => SubscriptionError::from(failure),
        SubscriptionForwardingError::JsonLd(error) => SubscriptionError::JsonLd(error),
        SubscriptionForwardingError::Representation(error) => SubscriptionError::Representation(error),
        other @ (SubscriptionForwardingError::Federation(_)
        | SubscriptionForwardingError::Registration(_)
        | SubscriptionForwardingError::Entity(_)
        | SubscriptionForwardingError::UnknownSubscription { .. }
        | SubscriptionForwardingError::Internal(_)) => {
            tracing::error!(error = %other, "the distributed side of a subscription failed");
            SubscriptionInvariant::DistributedSide.into()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{companion::companion_request, forwarder::ForwardDecision, receiver::ReceiverEndpoint, service::SubscriptionForwardingService};
    use andromeda_ngsi_ld_model::{
        registration::ContextSourceRegistration,
        subscription::{Subscription, SubscriptionType},
        subscription_request::SubscriptionRequest,
    };
    use andromeda_subscription::{compilation::compile_csource_subscription, forwarding_hook::ForwardedCopy};
    use serde_json::json;
    use url::Url;

    fn registration(entity_type: &str, mode: &str) -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:b",
            "type": "ContextSourceRegistration",
            "endpoint": "http://broker-b:1056",
            "mode": mode,
            "information": [{"entities": [{"type": entity_type}]}],
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid registration")
    }

    fn copy() -> ForwardedCopy {
        ForwardedCopy {
            registration: "urn:ngsi-ld:ContextSourceRegistration:b".parse().expect("valid URI"),
            remote_subscription: "urn:ngsi-ld:Subscription:forwarded-1".parse().expect("valid URI"),
        }
    }

    /// Clause 5.8.1.4: after an update, a source whose registration still
    /// matches is updated, one that no longer matches is relieved of its copy,
    /// and an auxiliary source never holds one.
    #[test]
    fn an_update_updates_matching_copies_and_deletes_the_rest() {
        let subscription: Subscription = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Subscription:companion",
            "type": "Subscription",
            "entities": [{"type": "https://example.org/Device"}],
            "notification": {"endpoint": {"uri": "http://broker-a/notify"}},
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid subscription");
        let compiled = compile_csource_subscription(&subscription);

        let still_matching = SubscriptionForwardingService::decision_after_update(&compiled, &registration("https://example.org/Device", "inclusive"), &copy());
        pretty_assertions::assert_eq!(
            still_matching,
            ForwardDecision::Update {
                remote: copy().remote_subscription
            }
        );

        let no_longer = SubscriptionForwardingService::decision_after_update(&compiled, &registration("https://example.org/Vehicle", "inclusive"), &copy());
        pretty_assertions::assert_eq!(
            no_longer,
            ForwardDecision::Delete {
                remote: copy().remote_subscription
            }
        );

        let auxiliary = SubscriptionForwardingService::decision_after_update(&compiled, &registration("https://example.org/Device", "auxiliary"), &copy());
        pretty_assertions::assert_eq!(auxiliary, ForwardDecision::Nothing);
    }

    #[test]
    fn the_companion_mirrors_the_selection_and_notifies_the_receiver() {
        let subscription: Subscription = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Subscription:local",
            "type": "Subscription",
            "entities": [{"type": "https://example.org/Vehicle"}],
            "watchedAttributes": ["https://example.org/speed"],
            "notification": {"attributes": ["https://example.org/speed"], "endpoint": {"uri": "http://subscriber/notify"}},
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid subscription");
        let receiver = ReceiverEndpoint::from_advertised(&Url::parse("http://broker-a:1056").expect("valid URL")).expect("receiver");
        // A create adds the identifier and the type to the companion request.
        let request = SubscriptionRequest {
            id: Some("urn:ngsi-ld:Subscription:forwarded-1".parse().expect("valid URI")),
            type_: Some(SubscriptionType),
            ..companion_request(&subscription, &receiver)
        };
        pretty_assertions::assert_eq!(
            serde_json::to_value(&request).expect("the request writes"),
            json!({
                "id": "urn:ngsi-ld:Subscription:forwarded-1",
                "type": "Subscription",
                "entities": [{"type": "https://example.org/Vehicle"}],
                "watchedAttributes": ["https://example.org/speed"],
                "notification": {
                    "attributes": ["https://example.org/speed"],
                    "endpoint": {"uri": "http://broker-a:1056/ngsi-ld/andromeda/v1/notifications", "accept": "application/json"}
                }
            })
        );
    }
}
