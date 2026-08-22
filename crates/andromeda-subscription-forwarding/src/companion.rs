//! The companion context source registration subscription of a local
//! subscription (clauses 5.8.1.4, 5.8.2.4 and 5.8.5.4).
//!
//! A local subscription that is not `localOnly` is mirrored by a registration
//! subscription over the same entities and attributes whose notifications reach
//! this broker's receiver; those notifications drive the reduced copies placed
//! at the matching context sources. An update follows the original, and a
//! deletion tears down the companion together with every forwarded copy.

use crate::{
    error::SubscriptionForwardingError,
    forwarder::{ForwardDecision, decide},
    mapping::ForwardedCopy,
    service::SubscriptionForwardingService,
};
use andromeda_federation::subscription::SubscriptionForward;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{notification::CsrTriggerReason, registration::ContextSourceRegistration, subscription::Subscription};
use andromeda_subscription::{
    compilation::compile_csource_subscription,
    compiled::CompiledCsrSubscription,
    csource_matching::csource_subscription_selects_registration,
    error::SubscriptionError,
    forwarding_hook::SubscriptionForwardingHook,
};
use futures::future::BoxFuture;
use serde_json::{Map, Value, json};
use tracing::{debug, warn};

/// The companion document mirroring `subscription` (clause 5.11.2): the same
/// entity selectors, watched attributes and notified attributes, matched
/// against registrations rather than entities, notifying this broker.
fn companion_document(subscription: &Subscription, minted: &Uri, receiver: &url::Url) -> Value {
    let mut document = Map::new();
    document.insert("id".to_string(), json!(minted));
    document.insert("type".to_string(), json!("Subscription"));
    if let Some(entities) = &subscription.selection.entities {
        document.insert("entities".to_string(), json!(entities));
    }
    if let Some(watched) = &subscription.selection.watched_attributes {
        document.insert("watchedAttributes".to_string(), json!(watched));
    }
    let mut notification = Map::new();
    if let Some(attributes) = &subscription.notification.attributes {
        notification.insert("attributes".to_string(), json!(attributes));
    }
    notification.insert("endpoint".to_string(), json!({"uri": receiver, "accept": "application/json"}));
    document.insert("notification".to_string(), Value::Object(notification));
    Value::Object(document)
}

/// Registrations read per round trip while the copies of a new subscription
/// are placed.
const REGISTRATION_PAGE: u32 = 200;

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
    async fn create_companion(&self, subscription: &Subscription, context: &ContextSpec) -> Result<Uri, SubscriptionForwardingError> {
        let minted = self
            .mint_subscription_id()
            .map_err(|error| SubscriptionForwardingError::internal(error.to_string()))?;
        let document = companion_document(subscription, &minted, self.receiver.url());
        let companion = self.csource_subscriptions.create(document, &ContextSpec::core_only(), None).await?;
        self.place_initial_copies(subscription, &companion, context).await?;
        self.mappings.link_companion(&subscription.id, &companion).await?;
        self.router.register(&companion, &self.tenant).await?;
        debug!(subscription = %subscription.id, %companion, "companion context source registration subscription created");
        Ok(companion)
    }

    /// The companion as the matcher its notifications are decided by.
    async fn compiled_companion(&self, companion: &Uri) -> Result<CompiledCsrSubscription, SubscriptionForwardingError> {
        Ok(compile_csource_subscription(&self.csource_subscriptions.get(companion).await?))
    }

    /// Place a reduced copy at every registered source the companion already
    /// selects (clauses 5.11.2.4 and 5.12), before the creation answers.
    ///
    /// The companion's own "newlyMatching" notification reports the same
    /// sources, but it is delivered after the creation has been answered, and
    /// a change a source sees in between would reach no copy. A source the
    /// notification names again already holds its copy and is left alone.
    async fn place_initial_copies(&self, subscription: &Subscription, companion: &Uri, context: &ContextSpec) -> Result<(), SubscriptionForwardingError> {
        let compiled = self.compiled_companion(companion).await?;
        let store = self.federation.registration_store();
        let mut after: Option<Uri> = None;
        loop {
            let page = store
                .find_all_active(after.as_ref(), REGISTRATION_PAGE)
                .await
                .map_err(|error| SubscriptionForwardingError::persistence(error.to_string()))?;
            let exhausted = page.len() < REGISTRATION_PAGE as usize;
            for registration in page {
                after = Some(registration.id.clone()); // clone: the cursor outlives the page
                if !csource_subscription_selects_registration(&compiled, &registration) {
                    continue;
                }
                let decision = decide(CsrTriggerReason::NewlyMatching, registration.access.mode.unwrap_or_default(), None);
                self.apply_forward_decision(subscription, &registration, decision, context).await?;
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
    async fn tear_down(&self, local: &Uri) -> Result<(), SubscriptionForwardingError> {
        for copy in self.mappings.forwarded_of(local).await? {
            self.tear_down_forwarded_copy(local, &copy).await;
        }
        if let Some(companion) = self.mappings.companion_of(local).await? {
            self.tear_down_companion(local, &companion).await;
        }
        Ok(())
    }

    /// Delete the copy of `local` at `copy.registration`'s source and forget it.
    async fn tear_down_forwarded_copy(&self, local: &Uri, copy: &ForwardedCopy) {
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
        if let Err(error) = self.mappings.remove_forwarded(local, &copy.registration).await {
            warn!(subscription = %local, registration = %copy.registration, %error, "forwarded subscription copy mapping could not be removed");
        }
        if let Err(error) = self.router.unregister(&copy.remote_subscription).await {
            warn!(subscription = %local, remote = %copy.remote_subscription, %error, "forwarded subscription copy route could not be removed");
        }
    }

    /// Delete the companion of `local` and unlink it.
    async fn tear_down_companion(&self, local: &Uri, companion: &Uri) {
        if let Err(error) = self.csource_subscriptions.delete(companion).await {
            warn!(subscription = %local, %companion, %error, "companion subscription could not be deleted");
        }
        if let Err(error) = self.mappings.unlink_companion(local).await {
            warn!(subscription = %local, %companion, %error, "companion subscription could not be unlinked");
        }
        if let Err(error) = self.router.unregister(companion).await {
            warn!(subscription = %local, %companion, %error, "companion subscription route could not be removed");
        }
    }

    /// Follow an update of `subscription` on its companion and on every
    /// forwarded copy (clause 5.8.2.4).
    async fn follow_update(&self, subscription: &Subscription, context: &ContextSpec) -> Result<(), SubscriptionForwardingError> {
        if is_local_only(subscription) {
            return self.tear_down(&subscription.id).await;
        }
        let companion = match self.mappings.companion_of(&subscription.id).await? {
            Some(companion) => {
                let mut fragment = companion_document(subscription, &companion, self.receiver.url());
                if let Some(object) = fragment.as_object_mut() {
                    object.remove("id");
                    object.remove("type");
                }
                self.csource_subscriptions.update(&companion, fragment, &ContextSpec::core_only(), None).await?;
                companion
            }
            None => {
                self.create_companion(subscription, context).await?;
                return Ok(());
            }
        };
        let compiled = self.compiled_companion(&companion).await?;
        for copy in self.mappings.forwarded_of(&subscription.id).await? {
            if let Some(registration) = self.federation_registration(&copy.registration).await? {
                let decision = Self::decision_after_update(&compiled, &registration, &copy);
                self.apply_forward_decision(subscription, &registration, decision, context).await?;
            }
        }
        Ok(())
    }
}

impl SubscriptionForwardingHook for SubscriptionForwardingService {
    fn created<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        subscription: &'life1 Subscription,
        context: &'life2 ContextSpec,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            if is_local_only(subscription) {
                return Ok(());
            }
            self.create_companion(subscription, context).await.map_err(into_subscription_error)?;
            Ok(())
        })
    }

    fn updated<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        subscription: &'life1 Subscription,
        context: &'life2 ContextSpec,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move { self.follow_update(subscription, context).await.map_err(into_subscription_error) })
    }

    fn deleted<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move { self.tear_down(id).await.map_err(into_subscription_error) })
    }
}

/// Report a distributed-side failure through the hook's error type.
fn into_subscription_error(error: SubscriptionForwardingError) -> SubscriptionError {
    match error {
        SubscriptionForwardingError::Subscription(inner) => inner,
        other @ (SubscriptionForwardingError::Federation(_)
        | SubscriptionForwardingError::Entity(_)
        | SubscriptionForwardingError::JsonLd(_)
        | SubscriptionForwardingError::Representation(_)
        | SubscriptionForwardingError::UnknownSubscription { .. }
        | SubscriptionForwardingError::Persistence { .. }
        | SubscriptionForwardingError::Internal { .. }) => SubscriptionError::persistence(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{companion::companion_document, forwarder::ForwardDecision, mapping::ForwardedCopy, service::SubscriptionForwardingService};
    use andromeda_ngsi_ld_model::{registration::ContextSourceRegistration, subscription::Subscription};
    use andromeda_subscription::compilation::compile_csource_subscription;
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
            remote_subscription: "urn:ngsi-ld:Subscription:forwarded:1".parse().expect("valid URI"),
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
        let minted = "urn:ngsi-ld:Subscription:forwarded:1".parse().expect("valid URI");
        let receiver = Url::parse("http://broker-a:1056/ngsi-ld/v1/notifications").expect("valid URL");
        let document = companion_document(&subscription, &minted, &receiver);
        pretty_assertions::assert_eq!(
            document,
            json!({
                "id": "urn:ngsi-ld:Subscription:forwarded:1",
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
