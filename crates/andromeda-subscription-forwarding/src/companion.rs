//! The companion context source registration subscription of a local
//! subscription (clauses 5.8.1.4, 5.8.2.4 and 5.8.5.4).
//!
//! A local subscription that is not `localOnly` is mirrored by a registration
//! subscription over the same entities and attributes whose notifications reach
//! this broker's receiver; those notifications drive the reduced copies placed
//! at the matching context sources. An update follows the original, and a
//! deletion tears down the companion together with every forwarded copy.
//!
//! A deletion is torn down from what was captured before the local row went:
//! the store removes the forwarding mappings together with the subscription,
//! so only the remote copies, the companion subscription and the platform
//! routes are left for this side to remove.

use crate::{
    error::SubscriptionForwardingError,
    forwarder::{ForwardDecision, decide},
    minted_subscription_id::mint_subscription_id,
    receiver::ReceiverEndpoint,
    service::SubscriptionForwardingService,
};
use andromeda_federation::subscription::SubscriptionForward;
use andromeda_jsonld::{context_spec::ContextSpec, user_context_admission::AdmittedContext};
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
use futures::{TryFutureExt, future::BoxFuture};
use tracing::{debug, warn};

/// The companion request mirroring `subscription` (clause 5.11.2): the same
/// entity selectors, watched attributes and notified attributes, matched
/// against registrations rather than entities, notifying this broker.
///
/// It names neither an identifier nor a type, so it is an update fragment as
/// it stands; a create adds both.
fn companion_request(subscription: &Subscription, receiver: &ReceiverEndpoint) -> SubscriptionRequest {
    SubscriptionRequest {
        entities: Clearable::from_option(subscription.selection.entities.clone()), // clone: the companion owns the selectors it mirrors
        watched_attributes: Clearable::from_option(subscription.selection.watched_attributes.clone()), // clone: the companion owns the names it mirrors
        notification: Some(NotificationRequest {
            attributes: subscription.notification.attributes.clone(), // clone: the companion owns the names it mirrors
            endpoint: Some(receiver.notification_endpoint()),
            ..NotificationRequest::default()
        }),
        ..SubscriptionRequest::default()
    }
}

/// Registrations read per round trip while the copies of a new subscription
/// are placed.
const REGISTRATION_PAGE: u32 = 200;

/// Where the mappings of the copies being torn down stand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CopyMappings {
    /// The local subscription was deleted, and its mappings with it.
    RemovedWithSubscription,
    /// The local subscription still exists, so each copy's mapping is removed
    /// here as the copy is.
    StillRecorded,
}

/// Whether a subscription takes part in distributed operations at all.
fn is_local_only(subscription: &Subscription) -> bool {
    subscription.delivery.local_only == Some(true)
}

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
        context: &ContextSpec,
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
        self.place_initial_copies(subscription, &companion, context, encountered_sources).await?;
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
        context: &ContextSpec,
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
                after = Some(registration.id.clone()); // clone: the cursor outlives the page
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
                self.apply_forward_decision(subscription, &registration, decision, &DeletedMembers::default(), context)
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
        decide(trigger, registration.access.mode.unwrap_or_default(), Some(copy.remote_subscription.clone())) // clone: the decision owns the copy's identifier
    }

    /// What tearing `local` down involves, read from the mappings while they
    /// still exist.
    async fn teardown_of(&self, local: &Identifier) -> Result<SubscriptionTeardown, SubscriptionForwardingError> {
        Ok(SubscriptionTeardown {
            companion: self.mappings.companion_of(local).await?,
            forwarded: self.mappings.forwarded_of(local).await?,
        })
    }

    /// Tear down every copy forwarded for `local` and then its companion
    /// (clause 5.8.5.4).
    ///
    /// The copies go first: while the companion still stands, a registration
    /// change notified through it could place a fresh copy at a source after
    /// that source's copy was removed, and a copy deleted before its mapping
    /// is forgotten cannot be re-created by a companion that no longer exists.
    /// Each step is attempted whatever the previous one did and a failure is
    /// reported per step, so one unreachable source never leaves the other
    /// copies, the mappings or the companion behind.
    async fn tear_down(&self, local: &Identifier, teardown: &SubscriptionTeardown, mappings: CopyMappings) {
        for copy in &teardown.forwarded {
            self.tear_down_forwarded_copy(local, copy, mappings).await;
        }
        if let Some(companion) = &teardown.companion {
            self.tear_down_companion(local, companion).await;
        }
    }

    /// Delete the copy of `local` at `copy.registration`'s source, forget its
    /// mapping unless the deletion of `local` already did, and drop its route.
    async fn tear_down_forwarded_copy(&self, local: &Identifier, copy: &ForwardedCopy, mappings: CopyMappings) {
        match self.federation_registration(&copy.registration).await {
            Ok(Some(registration)) => {
                let forwarded = self
                    .federation
                    .forward_subscription(
                        &registration,
                        SubscriptionForward::Delete {
                            remote_id: &copy.remote_subscription,
                        },
                    )
                    .await;
                if !forwarded.succeeded() {
                    warn!(subscription = %local, registration = %copy.registration, "forwarded subscription copy could not be deleted at the source");
                }
            }
            Ok(None) => debug!(subscription = %local, registration = %copy.registration, "registration of a forwarded copy is gone; dropping the mapping"),
            Err(error) => {
                warn!(subscription = %local, registration = %copy.registration, %error, "registration of a forwarded copy could not be read; the copy may remain at the source");
            }
        }
        match mappings {
            CopyMappings::StillRecorded => {
                if let Err(error) = self.mappings.remove_forwarded(local, &copy.registration).await {
                    warn!(subscription = %local, registration = %copy.registration, %error, "forwarded subscription copy mapping could not be removed");
                }
            }
            CopyMappings::RemovedWithSubscription => {}
        }
        if let Err(error) = self.router.unregister(&copy.remote_subscription).await {
            warn!(subscription = %local, remote = %copy.remote_subscription, %error, "forwarded subscription copy route could not be removed");
        }
    }

    /// Delete the companion of `local`, which removes its mapping with it, and
    /// drop its route.
    async fn tear_down_companion(&self, local: &Identifier, companion: &Identifier) {
        if let Err(error) = self.csource_subscriptions.delete(&self.live_target, companion).await {
            warn!(subscription = %local, %companion, %error, "companion subscription could not be deleted");
        }
        if let Err(error) = self.router.unregister(companion).await {
            warn!(subscription = %local, %companion, %error, "companion subscription route could not be removed");
        }
    }

    /// Follow an update of `subscription` on its companion and on every
    /// forwarded copy (clause 5.8.2.4).
    async fn follow_update(&self, subscription: &Subscription, deleted: &DeletedMembers, context: &ContextSpec) -> Result<(), SubscriptionForwardingError> {
        if is_local_only(subscription) {
            let teardown = self.teardown_of(&subscription.id).await?;
            self.tear_down(&subscription.id, &teardown, CopyMappings::StillRecorded).await;
            return Ok(());
        }
        let companion = if let Some(companion) = self.mappings.companion_of(&subscription.id).await? {
            self.csource_subscriptions
                .update(
                    &self.live_target,
                    &companion,
                    companion_request(subscription, &self.receiver),
                    &AdmittedContext::core_only(),
                )
                .await?;
            companion
        } else {
            // An update that first provisions the companion carries no
            // creation-time `Via` chain here; the update path forwards to
            // every selected source.
            self.create_companion(subscription, context, &[]).await?;
            return Ok(());
        };
        let compiled = self.compiled_companion(&companion).await?;
        for copy in self.mappings.forwarded_of(&subscription.id).await? {
            if let Some(registration) = self.federation_registration(&copy.registration).await? {
                let decision = Self::decision_after_update(&compiled, &registration, &copy);
                self.apply_forward_decision(subscription, &registration, decision, deleted, context).await?;
            }
        }
        Ok(())
    }
}

impl SubscriptionForwardingHook for SubscriptionForwardingService {
    fn created<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        subscription: &'life1 Subscription,
        context: &'life2 ContextSpec,
        encountered_sources: &'life3 [ContextSourcePseudonym],
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            if is_local_only(subscription) {
                return Ok(());
            }
            self.create_companion(subscription, context, encountered_sources)
                .await
                .map_err(into_subscription_error)?;
            Ok(())
        })
    }

    fn updated<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        subscription: &'life1 Subscription,
        deleted: &'life2 DeletedMembers,
        context: &'life3 ContextSpec,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait,
    {
        // The update future is large, so it is boxed as it stands rather than
        // moved into a wrapping async block.
        Box::pin(self.follow_update(subscription, deleted, context).map_err(into_subscription_error))
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
            self.tear_down(id, teardown, CopyMappings::RemovedWithSubscription).await;
            Ok(())
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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
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
                    "endpoint": {"uri": "http://broker-a:1056/ngsi-ld/v1/notifications", "accept": "application/json"}
                }
            })
        );
    }
}
