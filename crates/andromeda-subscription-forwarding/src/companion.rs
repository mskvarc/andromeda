//! The companion context source registration subscription of a local
//! subscription (clauses 5.8.1.4, 5.8.2.4 and 5.8.5.4).
//!
//! A local subscription that is not `localOnly` is mirrored by a registration
//! subscription over the same entities, attributes, context source filter and
//! `datasetId` whose notifications reach this broker's receiver; those notifications drive
//! the reduced copies placed at the matching context sources. An update
//! follows the original, and a deletion or an expiry tears down the companion
//! together with every forwarded copy.

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

/// The companion fragment mirroring `subscription` (clause 5.11.2): the same
/// entity selectors, watched attributes, notified attributes, context source
/// filter and `datasetId`, matched against registrations rather than
/// entities, notifying this broker.
///
/// The `csf` is mirrored because Table 5.2.12-1 has it "met by Context Source
/// Registrations describing Context Sources to be used for Entity
/// Subscriptions", and clause 5.11.2.4 matches a registration to the companion
/// only where it is met. The `datasetId` is mirrored because clause 5.12
/// matches a registration listing `datasetId`s only when it shares one, and
/// clause 4.3.6.1 has every constraint a registration specifies respected.
/// Each mirrored member the subscription does not hold
/// is the NGSI-LD Null, so an update leaves the companion holding what the
/// subscription holds and nothing it deleted.
fn companion_request(subscription: &Subscription, receiver: &ReceiverEndpoint) -> SubscriptionRequest {
    SubscriptionRequest {
        entities: Clearable::replacing(subscription.selection.entities.clone()),
        watched_attributes: Clearable::replacing(subscription.selection.watched_attributes.clone()),
        csf: Clearable::replacing(subscription.filters.csf.clone()),
        dataset_id: Clearable::replacing(subscription.filters.dataset_id.clone()),
        notification: Some(NotificationRequest {
            attributes: subscription.notification.attributes.clone(),
            endpoint: Some(receiver.notification_endpoint()),
            ..NotificationRequest::default()
        }),
        ..SubscriptionRequest::default()
    }
}

/// The create body of the companion mirroring `subscription` under the
/// identifier `id`: the companion fragment without its deletions, which a
/// create refuses (clause 5.2.1), with the identifier and the type.
fn companion_creation(subscription: &Subscription, receiver: &ReceiverEndpoint, id: Identifier) -> SubscriptionRequest {
    SubscriptionRequest {
        id: Some(id),
        type_: Some(SubscriptionType),
        ..companion_request(subscription, receiver).without_deletions()
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
        let request = companion_creation(subscription, &self.receiver, mint_subscription_id()?);
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
    use crate::{
        companion::{companion_creation, companion_request},
        forwarder::ForwardDecision,
        receiver::ReceiverEndpoint,
        service::SubscriptionForwardingService,
    };
    use andromeda_ngsi_ld_model::{registration::ContextSourceRegistration, subscription::Subscription};
    use andromeda_subscription::{compilation::compile_csource_subscription, forwarding_hook::ForwardedCopy};
    use serde_json::{Value, json};
    use url::Url;

    /// A stored context source filter selecting the registrations whose `tier`
    /// is "gold", its property term expanded.
    const GOLD_TIER: &str = r#""https://uri.etsi.org/ngsi-ld/default-context/tier"=="gold""#;

    fn registration(entity_type: &str, mode: &str) -> ContextSourceRegistration {
        registration_with(entity_type, mode, &json!({}))
    }

    /// A registration of `entity_type` in `mode` that also holds the members of
    /// `members`, an object.
    fn registration_with(entity_type: &str, mode: &str, members: &Value) -> ContextSourceRegistration {
        let mut document = json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:b",
            "type": "ContextSourceRegistration",
            "endpoint": "http://broker-b:1056",
            "mode": mode,
            "information": [{"entities": [{"type": entity_type}]}],
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        });
        if let (Some(target), Some(members)) = (document.as_object_mut(), members.as_object()) {
            target.extend(members.iter().map(|(key, value)| (key.clone(), value.clone())));
        }
        serde_json::from_value(document).expect("valid registration")
    }

    fn copy() -> ForwardedCopy {
        ForwardedCopy {
            registration: "urn:ngsi-ld:ContextSourceRegistration:b".parse().expect("valid URI"),
            remote_subscription: "urn:ngsi-ld:Subscription:forwarded-1".parse().expect("valid URI"),
        }
    }

    fn receiver() -> ReceiverEndpoint {
        ReceiverEndpoint::from_advertised(&Url::parse("http://broker-a:1056").expect("valid URL")).expect("receiver")
    }

    /// A local subscription holding the members of `members`, an object, beside
    /// its notification and its generated members.
    fn local_subscription(members: &Value) -> Subscription {
        let mut document = json!({
            "id": "urn:ngsi-ld:Subscription:local",
            "type": "Subscription",
            "notification": {"endpoint": {"uri": "http://subscriber/notify"}},
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        });
        if let (Some(target), Some(members)) = (document.as_object_mut(), members.as_object()) {
            target.extend(members.iter().map(|(key, value)| (key.clone(), value.clone())));
        }
        serde_json::from_value(document).expect("valid subscription")
    }

    /// Clause 5.8.1.4: after an update, a source whose registration still
    /// matches is updated, one that no longer matches is relieved of its copy,
    /// and an auxiliary source never holds one.
    #[test]
    fn an_update_updates_matching_copies_and_deletes_the_rest() {
        let subscription = local_subscription(&json!({"entities": [{"type": "https://example.org/Device"}]}));
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

    /// Table 5.2.12-1 and clause 5.11.2.4: a companion holding the
    /// subscription's `csf` keeps the copy at a source whose registration meets
    /// it and relieves one whose registration does not, or holds no `tier`.
    #[test]
    fn an_update_keeps_the_copies_only_at_the_sources_meeting_the_context_source_filter() {
        let companion = local_subscription(&json!({"entities": [{"type": "https://example.org/Device"}], "csf": GOLD_TIER}));
        let compiled = compile_csource_subscription(&companion);
        let tier = "https://uri.etsi.org/ngsi-ld/default-context/tier";
        for (members, expected) in [
            (
                json!({tier: {"type": "Property", "value": "gold"}}),
                ForwardDecision::Update {
                    remote: copy().remote_subscription,
                },
            ),
            (
                json!({tier: {"type": "Property", "value": "silver"}}),
                ForwardDecision::Delete {
                    remote: copy().remote_subscription,
                },
            ),
            (
                json!({}),
                ForwardDecision::Delete {
                    remote: copy().remote_subscription,
                },
            ),
        ] {
            let registration = registration_with("https://example.org/Device", "inclusive", &members);
            pretty_assertions::assert_eq!(
                SubscriptionForwardingService::decision_after_update(&compiled, &registration, &copy()),
                expected,
                "{members}"
            );
        }
    }

    /// Clause 5.12: a companion holding the subscription's `datasetId` keeps
    /// the copy at a source whose registration shares one or lists none, and
    /// relieves one whose registration lists only others.
    #[test]
    fn an_update_keeps_the_copies_only_at_the_sources_sharing_a_dataset_id() {
        let companion = local_subscription(&json!({"entities": [{"type": "https://example.org/Device"}], "datasetId": ["urn:ngsi-ld:Dataset:a"]}));
        let compiled = compile_csource_subscription(&companion);
        let kept = ForwardDecision::Update {
            remote: copy().remote_subscription,
        };
        let relieved = ForwardDecision::Delete {
            remote: copy().remote_subscription,
        };
        for (members, expected) in [
            (json!({"datasetId": ["urn:ngsi-ld:Dataset:b", "urn:ngsi-ld:Dataset:a"]}), &kept),
            (json!({}), &kept),
            (json!({"datasetId": ["urn:ngsi-ld:Dataset:b", "@none"]}), &relieved),
        ] {
            let registration = registration_with("https://example.org/Device", "inclusive", &members);
            pretty_assertions::assert_eq!(
                &SubscriptionForwardingService::decision_after_update(&compiled, &registration, &copy()),
                expected,
                "{members}"
            );
        }
    }

    /// Clauses 5.11.2 and 5.2.12: the companion is created with the
    /// subscription's selectors, notified attributes, `csf` and `datasetId`,
    /// notifying this broker, and with no NGSI-LD Null, which a create refuses.
    #[test]
    fn the_companion_mirrors_the_selection_and_notifies_the_receiver() {
        let subscription = local_subscription(&json!({
            "entities": [{"type": "https://example.org/Vehicle"}],
            "watchedAttributes": ["https://example.org/speed"],
            "csf": GOLD_TIER,
            "datasetId": ["urn:ngsi-ld:Dataset:a", "@none"],
            "notification": {"attributes": ["https://example.org/speed"], "endpoint": {"uri": "http://subscriber/notify"}}
        }));
        let request = companion_creation(&subscription, &receiver(), "urn:ngsi-ld:Subscription:forwarded-1".parse().expect("valid URI"));
        pretty_assertions::assert_eq!(
            serde_json::to_value(&request).expect("the request writes"),
            json!({
                "id": "urn:ngsi-ld:Subscription:forwarded-1",
                "type": "Subscription",
                "entities": [{"type": "https://example.org/Vehicle"}],
                "watchedAttributes": ["https://example.org/speed"],
                "csf": GOLD_TIER,
                "datasetId": ["urn:ngsi-ld:Dataset:a", "@none"],
                "notification": {
                    "attributes": ["https://example.org/speed"],
                    "endpoint": {"uri": "http://broker-a:1056/ngsi-ld/andromeda/v1/notifications", "accept": "application/json"}
                }
            })
        );
        let unfiltered = companion_creation(
            &local_subscription(&json!({"entities": [{"type": "https://example.org/Vehicle"}]})),
            &receiver(),
            "urn:ngsi-ld:Subscription:forwarded-2".parse().expect("valid URI"),
        );
        assert!(unfiltered.deleted_members().next().is_none(), "{unfiltered:?}");
    }

    /// Clause 5.5.8: the companion update deletes each mirrored member the
    /// subscription no longer holds, so a deleted `csf`, `datasetId` or
    /// `watchedAttributes` stops narrowing the registrations the companion
    /// matches.
    #[test]
    fn the_companion_update_deletes_what_the_subscription_no_longer_holds() {
        let subscription = local_subscription(&json!({"entities": [{"type": "https://example.org/Vehicle"}]}));
        let fragment = serde_json::to_value(companion_request(&subscription, &receiver())).expect("the fragment writes");
        pretty_assertions::assert_eq!(fragment["entities"], json!([{"type": "https://example.org/Vehicle"}]));
        pretty_assertions::assert_eq!(fragment["watchedAttributes"], json!(["urn:ngsi-ld:null"]));
        pretty_assertions::assert_eq!(fragment["csf"], json!("urn:ngsi-ld:null"));
        pretty_assertions::assert_eq!(fragment["datasetId"], json!(["urn:ngsi-ld:null"]));
        let filtered =
            local_subscription(&json!({"watchedAttributes": ["https://example.org/speed"], "csf": GOLD_TIER, "datasetId": ["urn:ngsi-ld:Dataset:a"]}));
        let fragment = serde_json::to_value(companion_request(&filtered, &receiver())).expect("the fragment writes");
        pretty_assertions::assert_eq!(fragment["entities"], json!(["urn:ngsi-ld:null"]));
        pretty_assertions::assert_eq!(fragment["csf"], json!(GOLD_TIER));
        pretty_assertions::assert_eq!(fragment["datasetId"], json!(["urn:ngsi-ld:Dataset:a"]));
    }
}
