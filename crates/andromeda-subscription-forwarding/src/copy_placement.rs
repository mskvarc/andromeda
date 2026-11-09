//! Placing one reduced copy at a context source (clause 5.8.1.4): claimed
//! before it is sent, then kept, released or re-recorded by what the source
//! answered.

use crate::{error::SubscriptionForwardingError, forwarded_claim::ForwardedClaim, service::SubscriptionForwardingService};
use andromeda_federation::{
    remote_effect::RemoteEffect,
    subscription::{SubscriptionForward, SubscriptionForwardAdmission},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{registration::ContextSourceRegistration, subscription::Subscription};
use andromeda_subscription::forwarding_hook::ForwardedCopy;
use tracing::{debug, warn};

impl SubscriptionForwardingService {
    /// Place the copy `create` carries at `registration`'s source under
    /// `minted`.
    ///
    /// The copy is recorded and its identifier routed before it is sent, so a
    /// notification the source sends under it the moment it holds it finds
    /// both. A failure that left nothing at the source releases them. A forward
    /// that got no answer keeps them, since the source may hold the copy and
    /// notify under it. A source that assigned another identifier has the
    /// copy recorded and routed under that one.
    ///
    /// # Errors
    ///
    /// Propagates the mapping-store and router failures.
    pub(crate) async fn place_copy(
        &self,
        subscription: &Subscription,
        registration: &ContextSourceRegistration,
        minted: &Identifier,
        create: SubscriptionForward<'_>,
    ) -> Result<(), SubscriptionForwardingError> {
        if create.admission(registration) == SubscriptionForwardAdmission::NotAdvertised {
            debug!(subscription = %subscription.id, registration = %registration.id, "registration does not advertise createSubscription");
            return Ok(());
        }
        let claim = ForwardedCopy {
            registration: registration.id.clone(),
            remote_subscription: minted.clone(), // clone: the claim owns the identifier the route and the forward still name
        };
        match self.mappings.claim_forwarded(&subscription.id, &claim).await? {
            ForwardedClaim::Claimed => {}
            ForwardedClaim::Declined => {
                debug!(subscription = %subscription.id, registration = %registration.id, "a copy is already recorded at the registration, or either no longer exists");
                return Ok(());
            }
        }
        if let Err(error) = self.router.register(minted, &self.tenant).await {
            self.mappings.remove_forwarded(&subscription.id, &registration.id).await?;
            return Err(error);
        }
        let forwarded = self.federation.forward_subscription(registration, create).await;
        let Some(failure) = forwarded.failure() else {
            return self
                .adopt_assigned(&subscription.id, registration, minted, &forwarded.assigned_id(minted))
                .await;
        };
        match failure.remote_effect() {
            RemoteEffect::Possible => {
                warn!(subscription = %subscription.id, registration = %registration.id, %failure, "reduced subscription copy got no answer from the source and stays recorded, since the source may hold it");
                Ok(())
            }
            RemoteEffect::Untouched => {
                warn!(subscription = %subscription.id, registration = %registration.id, %failure, "reduced subscription copy was not accepted by the source");
                self.router.unregister(minted).await?;
                self.mappings.remove_forwarded(&subscription.id, &registration.id).await
            }
        }
    }

    /// Record and route the copy of `local` at `registration` under
    /// `assigned`, when the source assigned it in place of `minted`.
    async fn adopt_assigned(
        &self,
        local: &Identifier,
        registration: &ContextSourceRegistration,
        minted: &Identifier,
        assigned: &Identifier,
    ) -> Result<(), SubscriptionForwardingError> {
        if assigned == minted {
            return Ok(());
        }
        self.mappings
            .record_forwarded(
                local,
                &ForwardedCopy {
                    registration: registration.id.clone(),
                    remote_subscription: assigned.clone(), // clone: the route registers the same identifier after the mapping takes its copy
                },
            )
            .await?;
        self.router.register(assigned, &self.tenant).await?;
        self.router.unregister(minted).await
    }
}
