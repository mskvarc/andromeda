//! Placing, updating and removing reduced copies at context sources as the
//! companion's notifications report which registrations match
//! (clause 5.8.1.4, `localOnly=false`).

use crate::{
    error::SubscriptionForwardingError,
    inbound::InboundCsourceNotification,
    minted_subscription_id::mint_subscription_id,
    reduced_copy::{FilterCarriage, ReducedSubscription},
    service::SubscriptionForwardingService,
};
use andromeda_federation::subscription::{SubscriptionForward, SubscriptionForwardAdmission};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    notification::CsrTriggerReason,
    received::ReceivedElement,
    registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode},
    subscription::Subscription,
};
use andromeda_subscription::{deleted_members::DeletedMembers, forwarding_hook::ForwardedCopy};
use tracing::{debug, warn};

/// What one registration named in a companion notification calls for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ForwardDecision {
    /// Place a new reduced copy at the source.
    Create,
    /// Update the copy the source already holds.
    Update {
        /// The identifier of the copy at the source.
        remote: Identifier,
    },
    /// Remove the copy the source holds.
    Delete {
        /// The identifier of the copy at the source.
        remote: Identifier,
    },
    /// Nothing to do: an auxiliary registration, or a change to a source
    /// holding no copy.
    Nothing,
}

/// Decide what a trigger reason calls for, given whether a copy already exists
/// at the registration's source (clause 5.8.1.4).
pub(crate) fn decide(trigger: CsrTriggerReason, mode: ForwardingMode, existing: Option<Identifier>) -> ForwardDecision {
    // Clause 5.8.1.4 names exclusive, redirect and inclusive registrations;
    // an auxiliary source never receives a subscription.
    if mode == ForwardingMode::Auxiliary {
        return ForwardDecision::Nothing;
    }
    match (trigger, existing) {
        // A registration that changed into matching is, for this broker, newly matching.
        (CsrTriggerReason::NewlyMatching | CsrTriggerReason::Updated, None) => ForwardDecision::Create,
        (CsrTriggerReason::Updated, Some(remote)) => ForwardDecision::Update { remote },
        (CsrTriggerReason::NoLongerMatching, Some(remote)) => ForwardDecision::Delete { remote },
        // The copy was placed when the subscription was created (or the
        // registration was already reported): a source that holds one is
        // neither given a second one nor sent an update it has no change for,
        // and a source that holds none has nothing to be relieved of.
        (CsrTriggerReason::NewlyMatching, Some(_)) | (CsrTriggerReason::NoLongerMatching, None) => ForwardDecision::Nothing,
    }
}

impl SubscriptionForwardingService {
    /// Look a registration up by identifier.
    pub(crate) async fn federation_registration(&self, registration: &Identifier) -> Result<Option<ContextSourceRegistration>, SubscriptionForwardingError> {
        self.federation
            .registration_store()
            .get(registration)
            .await
            .map_err(SubscriptionForwardingError::from)
    }

    /// Carry out one decision for `subscription` at `registration`; `deleted`
    /// names the members the local update removed, which an update of the
    /// copy sends as deletions.
    pub(crate) async fn apply_forward_decision(
        &self,
        subscription: &Subscription,
        registration: &ContextSourceRegistration,
        decision: ForwardDecision,
        deleted: &DeletedMembers,
        context: &ContextSpec,
    ) -> Result<(), SubscriptionForwardingError> {
        let carriage = FilterCarriage::for_split(self.federation.split_entities(subscription.delivery.split_entities));
        match decision {
            ForwardDecision::Create => {
                let minted = mint_subscription_id()?;
                // Boxed: the copy is held across the forward, and the request is a wide value.
                let copy = Box::new(ReducedSubscription::reduce(subscription, registration, &self.receiver, &minted, carriage).request);
                let forwarded = self
                    .federation
                    .forward_subscription(registration, SubscriptionForward::Create { body: &copy, context })
                    .await;
                if forwarded.admission == SubscriptionForwardAdmission::NotAdvertised {
                    debug!(subscription = %subscription.id, registration = %registration.id, "registration does not advertise createSubscription");
                    return Ok(());
                }
                if !forwarded.succeeded() {
                    warn!(subscription = %subscription.id, registration = %registration.id, "reduced subscription copy was not accepted by the source");
                    return Ok(());
                }
                let remote = forwarded.assigned_id(&minted);
                self.mappings
                    .record_forwarded(
                        &subscription.id,
                        &ForwardedCopy {
                            registration: registration.id.clone(), // clone: the mapping owns the registration id
                            remote_subscription: remote.clone(),   // clone: the mapping and the route each own the id
                        },
                    )
                    .await?;
                self.router.register(&remote, &self.tenant).await?;
            }
            ForwardDecision::Update { remote } => {
                // Boxed: the fragment is held across the forward, and the request is a wide value.
                let fragment =
                    Box::new(ReducedSubscription::reduce(subscription, registration, &self.receiver, &remote, carriage).as_fragment(deleted, carriage));
                let forwarded = self
                    .federation
                    .forward_subscription(
                        registration,
                        SubscriptionForward::Update {
                            remote_id: &remote,
                            body: &fragment,
                            context,
                        },
                    )
                    .await;
                if forwarded.admission == SubscriptionForwardAdmission::Forwarded && !forwarded.succeeded() {
                    warn!(subscription = %subscription.id, registration = %registration.id, "reduced subscription copy could not be updated at the source");
                }
            }
            ForwardDecision::Delete { remote } => {
                let forwarded = self
                    .federation
                    .forward_subscription(registration, SubscriptionForward::Delete { remote_id: &remote })
                    .await;
                if forwarded.admission == SubscriptionForwardAdmission::Forwarded && !forwarded.succeeded() {
                    warn!(subscription = %subscription.id, registration = %registration.id, "reduced subscription copy could not be deleted at the source");
                }
                self.mappings.remove_forwarded(&subscription.id, &registration.id).await?;
                self.router.unregister(&remote).await?;
            }
            ForwardDecision::Nothing => {}
        }
        Ok(())
    }

    /// Handle a notification of a companion subscription: every registration it
    /// names is given, updated on, or relieved of a reduced copy.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionForwardingError::UnknownSubscription`] when the
    /// notification names a companion this broker holds no local subscription
    /// for. Propagates the mapping-store, federation, subscription-store and
    /// serialisation failures raised while placing, updating or removing the
    /// reduced copies the notification calls for.
    pub async fn handle_csource_notification(
        &self,
        notification: &InboundCsourceNotification,
        context: &ContextSpec,
    ) -> Result<(), SubscriptionForwardingError> {
        let Some(local) = self.mappings.local_of_companion(&notification.subscription_id).await? else {
            return Err(SubscriptionForwardingError::UnknownSubscription {
                subscription: notification.subscription_id.clone(), // clone: the error owns the identifier
            });
        };
        let Some(stored) = self.subscriptions.get(&self.live_target, &local).await? else {
            debug!(subscription = %local, "companion notification for a subscription that no longer exists");
            return Ok(());
        };
        let subscription = stored.subscription;
        let Some(trigger) = notification.trigger_reason else {
            return Ok(());
        };
        // The copies the local subscription holds are read once: the
        // registrations one notification names are distinct, so a decision
        // taken for one leaves the copy of another as it was read.
        let copies = self.mappings.forwarded_of(&local).await?;
        for element in &notification.data {
            let registration = match element {
                ReceivedElement::Read(registration) => registration,
                ReceivedElement::Refused(refusal) => {
                    warn!(subscription = %local, %refusal, "companion notification carried a registration that does not parse");
                    continue;
                }
            };
            let existing = copies
                .iter()
                .find(|copy| copy.registration == registration.id)
                .map(|copy| copy.remote_subscription.clone()); // clone: the decision owns the identifier of the copy it acts on
            let decision = decide(trigger, registration.access.mode.unwrap_or_default(), existing);
            // A registration change deletes nothing of the subscription.
            self.apply_forward_decision(&subscription, registration, decision, &DeletedMembers::default(), context)
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::forwarder::{ForwardDecision, decide};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{notification::CsrTriggerReason, registration::forwarding_mode::ForwardingMode};

    fn remote() -> Identifier {
        "urn:ngsi-ld:Subscription:forwarded-1".parse().expect("valid URI")
    }

    #[test]
    fn the_decision_table_follows_clause_5_8_1_4() {
        for mode in [ForwardingMode::Inclusive, ForwardingMode::Exclusive, ForwardingMode::Redirect] {
            pretty_assertions::assert_eq!(decide(CsrTriggerReason::NewlyMatching, mode, None), ForwardDecision::Create);
            pretty_assertions::assert_eq!(decide(CsrTriggerReason::NewlyMatching, mode, Some(remote())), ForwardDecision::Nothing);
            pretty_assertions::assert_eq!(
                decide(CsrTriggerReason::Updated, mode, Some(remote())),
                ForwardDecision::Update { remote: remote() }
            );
            pretty_assertions::assert_eq!(decide(CsrTriggerReason::Updated, mode, None), ForwardDecision::Create);
            pretty_assertions::assert_eq!(
                decide(CsrTriggerReason::NoLongerMatching, mode, Some(remote())),
                ForwardDecision::Delete { remote: remote() }
            );
            pretty_assertions::assert_eq!(decide(CsrTriggerReason::NoLongerMatching, mode, None), ForwardDecision::Nothing);
        }
    }

    #[test]
    fn an_auxiliary_registration_never_receives_a_subscription() {
        for trigger in [CsrTriggerReason::NewlyMatching, CsrTriggerReason::Updated, CsrTriggerReason::NoLongerMatching] {
            pretty_assertions::assert_eq!(decide(trigger, ForwardingMode::Auxiliary, Some(remote())), ForwardDecision::Nothing);
        }
    }
}
