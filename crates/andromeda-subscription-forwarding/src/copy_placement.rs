//! Placing one reduced copy at a context source (clause 5.8.1.4), claimed
//! before it is sent, then settled, released or recorded again by what the
//! source answered.

use crate::{
    error::SubscriptionForwardingError,
    forwarded_claim::{ClaimOrigin, ForwardedClaim},
    minted_subscription_id::mint_subscription_id,
    reduced_copy::{FilterCarriage, ReducedSubscription},
    service::SubscriptionForwardingService,
};
use andromeda_federation::{
    answer_verdict::AnswerVerdict,
    remote_effect::RemoteEffect,
    subscription::{SubscriptionForward, SubscriptionForwardAdmission},
    subscription_forward_failure::SubscriptionForwardFailure,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{registration::ContextSourceRegistration, subscription::Subscription};
use andromeda_subscription::forwarding_hook::ForwardedCopy;
use tracing::{debug, warn};

/// What a forward that left nothing at the source does to the claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UntouchedClaim {
    /// The claim and the route are released.
    Release,
    /// The claim and the route stay to the lease, and a later pass sends the
    /// copy again.
    KeepToLease,
}

/// What `failure`, which left nothing at the source, does to a claim of
/// `origin`.
///
/// A claim recorded with its copy had nothing sent before it, so the source
/// holds nothing and the claim is released. A claim taken over may name a copy
/// an earlier send left at the source. A source that refuses the copy as sent
/// refused that earlier send too, since one holding the copy answers
/// `AlreadyExists`, so the claim is released. An answer that depends on the
/// moment it came (an unavailable source, a rate limit) and a forward that
/// never reached the source (a cooldown, a refused connection, a request that
/// could not be prepared, a loop) learned nothing of the earlier send, so the
/// claim stays.
fn untouched_claim(origin: ClaimOrigin, failure: &SubscriptionForwardFailure) -> UntouchedClaim {
    match (origin, failure) {
        (ClaimOrigin::TakenOver, SubscriptionForwardFailure::Answered(answer)) => match answer.verdict() {
            AnswerVerdict::Refused => UntouchedClaim::Release,
            AnswerVerdict::Withheld => UntouchedClaim::KeepToLease,
        },
        (
            ClaimOrigin::TakenOver,
            SubscriptionForwardFailure::Transport(_) | SubscriptionForwardFailure::LoopDetected | SubscriptionForwardFailure::NotAdvertised,
        ) => UntouchedClaim::KeepToLease,
        (
            ClaimOrigin::Recorded,
            SubscriptionForwardFailure::Answered(_)
            | SubscriptionForwardFailure::Transport(_)
            | SubscriptionForwardFailure::LoopDetected
            | SubscriptionForwardFailure::NotAdvertised,
        ) => UntouchedClaim::Release,
    }
}

impl SubscriptionForwardingService {
    /// Place a reduced copy of `subscription` at `registration`'s source, with
    /// its filters carried as `carriage` and its terms written under `context`.
    ///
    /// The copy is claimed under a lease and its identifier routed before it is
    /// sent, so a notification the source sends under it the moment it holds it
    /// finds both. A claim that took over an abandoned one sends the copy under
    /// the identifier recorded. An answer saying the source holds the copy
    /// settles the claim, under an identifier the source assigned when it
    /// assigned one. A source refusing the copy as sent releases the claim and
    /// the route. An error status that depends on the moment it came, and a
    /// forward that never reached the source, release them too when the claim
    /// was recorded with the copy, and leave them to the lease when the claim
    /// was taken over, since an earlier send may have left the copy there. A
    /// forward that got no answer, or only a gateway's saying the source gave
    /// none in time, keeps both under the lease, since the source may hold the
    /// copy and notify under it, and the copy is sent again once the lease runs
    /// out. A route that cannot be written
    /// leaves the claim to its lease too, as a placement cut off there would.
    ///
    /// # Errors
    ///
    /// Propagates the mapping-store and router failures, and the failure to
    /// mint an identifier.
    pub(crate) async fn place_copy(
        &self,
        subscription: &Subscription,
        registration: &ContextSourceRegistration,
        carriage: FilterCarriage,
        context: &ContextSpec,
    ) -> Result<(), SubscriptionForwardingError> {
        if SubscriptionForwardAdmission::of_creation(registration) == SubscriptionForwardAdmission::NotAdvertised {
            debug!(subscription = %subscription.id, registration = %registration.id, "registration does not advertise createSubscription");
            return Ok(());
        }
        let candidate = ForwardedCopy {
            registration: registration.id.clone(),
            remote_subscription: mint_subscription_id()?,
        };
        let (remote, origin) = match self.mappings.claim_forwarded(&subscription.id, &candidate, self.placement_lease).await? {
            ForwardedClaim::Claimed { remote, origin } => (remote, origin),
            ForwardedClaim::Declined => {
                debug!(subscription = %subscription.id, registration = %registration.id, "a copy is already recorded at the registration, or either no longer exists");
                return Ok(());
            }
        };
        self.router.register(&remote, &self.tenant).await?;
        // Boxed, since the copy is held across the forward and the request is a wide value.
        let copy = Box::new(ReducedSubscription::reduce(subscription, registration, &self.receiver, &remote, carriage).request);
        let forwarded = self
            .federation
            .forward_subscription(registration, SubscriptionForward::Create { body: &copy, context })
            .await;
        let Some(failure) = forwarded.failure() else {
            return self.settle_held(&subscription.id, registration, &remote, &forwarded.assigned_id(&remote)).await;
        };
        match failure.remote_effect() {
            RemoteEffect::Present => {
                debug!(subscription = %subscription.id, registration = %registration.id, %failure, "the source already holds the reduced subscription copy");
                self.mappings.settle_forwarded(&subscription.id, &registration.id).await
            }
            RemoteEffect::Possible => {
                warn!(subscription = %subscription.id, registration = %registration.id, %failure, "reduced subscription copy got no answer from the source and stays claimed, since the source may hold it; it is sent again once its lease runs out");
                Ok(())
            }
            RemoteEffect::Untouched => match untouched_claim(origin, &failure) {
                UntouchedClaim::Release => {
                    warn!(subscription = %subscription.id, registration = %registration.id, %failure, "reduced subscription copy was not accepted by the source");
                    self.router.unregister(&remote).await?;
                    self.mappings.remove_forwarded(&subscription.id, &registration.id).await
                }
                UntouchedClaim::KeepToLease => {
                    warn!(subscription = %subscription.id, registration = %registration.id, %failure, "reduced subscription copy sent again got no verdict from the source and stays claimed, since an earlier send may have left it there; it is sent again once its lease runs out");
                    Ok(())
                }
            },
        }
    }

    /// Settle the copy of `local` that `registration`'s source accepted under
    /// `assigned`, after it was sent under `claimed`. A source that assigned
    /// another identifier notifies under that one (clause 5.2.12), so the copy
    /// is recorded and routed under it and `claimed` is forgotten.
    async fn settle_held(
        &self,
        local: &Identifier,
        registration: &ContextSourceRegistration,
        claimed: &Identifier,
        assigned: &Identifier,
    ) -> Result<(), SubscriptionForwardingError> {
        if assigned == claimed {
            return self.mappings.settle_forwarded(local, &registration.id).await;
        }
        self.mappings
            .record_forwarded(
                local,
                &ForwardedCopy {
                    registration: registration.id.clone(),
                    remote_subscription: assigned.clone(),
                },
            )
            .await?;
        self.router.register(assigned, &self.tenant).await?;
        self.router.unregister(claimed).await
    }
}
