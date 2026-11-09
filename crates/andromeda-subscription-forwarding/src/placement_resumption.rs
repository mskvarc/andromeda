//! Placing again the reduced copies whose claim was abandoned before the
//! source answered (clause 5.8.1.4).
//!
//! A placement runs inside the request or notification that started it, so
//! the request deadline, a shutdown or a crash can cut it off between its
//! claim and the source's answer, and its claim then outlives its lease. The
//! tenant's background pass reads such claims and places each copy again,
//! which takes the claim over and sends the copy under the identifier it was
//! recorded with. A source therefore ends up holding the copy whatever point
//! the first placement was cut off at.

use crate::{
    error::SubscriptionForwardingError,
    forwarded_claim::StaleClaim,
    forwarder::{ForwardDecision, decide},
    participation::stays_local,
    service::SubscriptionForwardingService,
};
use andromeda_federation::subscription::SubscriptionForwardAdmission;
use andromeda_ngsi_ld_model::notification::CsrTriggerReason;
use andromeda_subscription::deleted_members::DeletedMembers;
use chrono::Utc;
use tracing::{debug, warn};

/// The abandoned claims one pass places again at most. The passes that follow
/// work through a longer backlog.
const RESUMPTION_PAGE: u32 = 100;

impl SubscriptionForwardingService {
    /// Place again every copy whose claim outlived its lease, up to
    /// [`RESUMPTION_PAGE`] of them, the longest overdue first.
    ///
    /// A copy that cannot be placed again is logged and left to its claim, and
    /// the pass goes on with the next one.
    ///
    /// # Errors
    ///
    /// Propagates the mapping-store failure to read the abandoned claims.
    pub async fn resume_stale_placements(&self) -> Result<(), SubscriptionForwardingError> {
        for claim in self.mappings.stale_claims(RESUMPTION_PAGE).await? {
            if let Err(error) = Box::pin(self.resume_placement(&claim)).await {
                warn!(subscription = %claim.local, registration = %claim.registration, %error, "an abandoned reduced subscription copy could not be placed again");
            }
        }
        Ok(())
    }

    /// Place the copy `claim` names again, as one lifecycle step of its local
    /// subscription.
    ///
    /// A subscription out of distributed operations is left alone, since its
    /// expiry withdraws every copy it holds. A subscription or registration
    /// that no longer exists has taken its claims with it. A registration that
    /// takes no copy any more, being auxiliary or no longer advertising the
    /// creation of subscriptions, has the copy withdrawn.
    async fn resume_placement(&self, claim: &StaleClaim) -> Result<(), SubscriptionForwardingError> {
        let _step = self.lifecycle_steps.async_lock(claim.local.clone()).await;
        let Some(stored) = self.subscriptions.get(&self.live_target, &claim.local).await? else {
            debug!(subscription = %claim.local, "an abandoned copy claim names a subscription that no longer exists");
            return Ok(());
        };
        if stays_local(&stored.subscription, Utc::now()) {
            debug!(subscription = %claim.local, "an abandoned copy claim names a subscription out of distributed operations");
            return Ok(());
        }
        let Some(registration) = self.federation_registration(&claim.registration).await? else {
            debug!(subscription = %claim.local, registration = %claim.registration, "an abandoned copy claim names a registration that no longer exists");
            return Ok(());
        };
        // Decided as for a registration found matching with no copy at its
        // source. One that takes no copy any more (an auxiliary one, or one
        // that no longer advertises the creation of subscriptions) has the
        // copy withdrawn, since its source may hold it, and the claim goes
        // with it rather than staying overdue.
        let takes_copies = SubscriptionForwardAdmission::of_creation(&registration);
        let decision = match (
            decide(CsrTriggerReason::NewlyMatching, registration.access.mode.unwrap_or_default(), None),
            takes_copies,
        ) {
            (ForwardDecision::Create, SubscriptionForwardAdmission::Forwarded) => ForwardDecision::Create,
            (ForwardDecision::Create, SubscriptionForwardAdmission::NotAdvertised)
            | (
                ForwardDecision::Update { .. } | ForwardDecision::Delete { .. } | ForwardDecision::Nothing,
                SubscriptionForwardAdmission::Forwarded | SubscriptionForwardAdmission::NotAdvertised,
            ) => ForwardDecision::Delete { remote: claim.remote.clone() },
        };
        self.apply_forward_decision(&stored.subscription, &registration, decision, &DeletedMembers::default())
            .await
    }
}
