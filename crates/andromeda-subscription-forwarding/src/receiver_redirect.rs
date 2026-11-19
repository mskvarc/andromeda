//! Bringing the companions and copies that name an earlier receiver to the
//! receiver this broker serves.
//!
//! Clause 5.8.1.4 sets the notification endpoint of a copy "to that of the
//! local Broker", and the companion notifies the same receiver. A receiver
//! that changed under a placed companion or copy leaves it notifying a path
//! this broker no longer serves, so every notification it sends is refused
//! and the local subscription hears nothing from that source. The tenant
//! adopts the current receiver before it serves a request, which makes every
//! companion and copy placed before a change owe the new one, and its
//! background pass updates each one owed.
//!
//! A companion is updated as an update of the local subscription updates it.
//! A copy is updated at its source as a registration change updates it, the
//! whole reduction sent again with the receiver in its notification endpoint.
//! A source that never takes that update, because it refuses it as sent or
//! its registration does not advertise the update of subscriptions, has the
//! copy withdrawn and placed again under a new identifier. A source that gives
//! no verdict is sent the update again after [`REDIRECT_RETRY`].

use crate::{
    copy_update::CopyUpdate,
    error::SubscriptionForwardingError,
    forwarder::{ForwardDecision, decide},
    participation::stays_local,
    redirect_ledger::{OwedRedirect, RedirectTarget},
    service::SubscriptionForwardingService,
};
use andromeda_ngsi_ld_model::{notification::CsrTriggerReason, registration::ContextSourceRegistration, subscription::Subscription};
use andromeda_subscription::{deleted_members::DeletedMembers, forwarding_hook::ForwardedCopy};
use chrono::Utc;
use std::time::Duration;
use tracing::{debug, warn};

/// The companions, and separately the copies, one pass updates at most. The
/// passes that follow work through a longer backlog.
const REDIRECT_PAGE: u32 = 100;

/// How long after an update that got no verdict the companion or copy is
/// updated again.
pub(crate) const REDIRECT_RETRY: Duration = Duration::from_secs(60);

impl SubscriptionForwardingService {
    /// Adopt the receiver this broker serves as the one the tenant's
    /// companions and copies name, before the tenant serves a request.
    ///
    /// # Errors
    ///
    /// Propagates the redirect ledger's failure.
    pub async fn adopt_receiver(&self) -> Result<(), SubscriptionForwardingError> {
        self.redirects.adopt_receiver(&self.receiver).await
    }

    /// Update every companion and copy whose update to the current receiver
    /// has fallen due, up to [`REDIRECT_PAGE`] of each, the longest overdue
    /// first.
    ///
    /// One that cannot be updated is logged and made due again after
    /// [`REDIRECT_RETRY`], and the pass goes on with the next one.
    ///
    /// # Errors
    ///
    /// Propagates the redirect ledger's failure to read the overdue updates.
    pub async fn redirect_overdue(&self) -> Result<(), SubscriptionForwardingError> {
        for owed in self.redirects.overdue_redirects(REDIRECT_PAGE).await? {
            if let Err(error) = Box::pin(self.redirect(&owed)).await {
                warn!(subscription = %owed.local, target = ?owed.target, %error, "a companion or copy owed the current receiver could not be updated");
                if let Err(error) = self.redirects.postpone_redirect(&owed.local, &owed.target, REDIRECT_RETRY).await {
                    warn!(subscription = %owed.local, target = ?owed.target, %error, "the update owed the current receiver could not be postponed");
                }
            }
        }
        Ok(())
    }

    /// Update what `owed` names to the current receiver, as one lifecycle step
    /// of its local subscription.
    ///
    /// A subscription that no longer exists has taken its companion and copies
    /// with it. One out of distributed operations has them withdrawn by its
    /// expiry, so the update is postponed rather than sent.
    async fn redirect(&self, owed: &OwedRedirect) -> Result<(), SubscriptionForwardingError> {
        let _step = self.lifecycle_steps.async_lock(owed.local.clone()).await;
        let Some(stored) = self.subscriptions.get(&self.live_target, &owed.local).await? else {
            debug!(subscription = %owed.local, "a redirect names a subscription that no longer exists");
            return Ok(());
        };
        if stays_local(&stored.subscription, Utc::now()) {
            debug!(subscription = %owed.local, "a redirect names a subscription out of distributed operations");
            return self.redirects.postpone_redirect(&owed.local, &owed.target, REDIRECT_RETRY).await;
        }
        match &owed.target {
            RedirectTarget::Companion => self.redirect_companion(&stored.subscription).await,
            RedirectTarget::Copy(copy) => self.redirect_copy(&stored.subscription, copy).await,
        }
    }

    /// Update the companion of `subscription` to notify the current receiver.
    /// A companion the store no longer holds is provisioned again, notifying
    /// that receiver, with the copies it calls for.
    async fn redirect_companion(&self, subscription: &Subscription) -> Result<(), SubscriptionForwardingError> {
        if self.update_companion(subscription).await?.is_none() {
            self.create_companion(subscription, &[]).await?;
        }
        Ok(())
    }

    /// Update `copy` of `subscription` at its source to notify the current
    /// receiver.
    async fn redirect_copy(&self, subscription: &Subscription, copy: &ForwardedCopy) -> Result<(), SubscriptionForwardingError> {
        let Some(registration) = self.federation_registration(&copy.registration).await? else {
            debug!(subscription = %subscription.id, registration = %copy.registration, "a redirect names a registration that no longer exists");
            return Ok(());
        };
        let form = self.copy_form(subscription);
        match self
            .update_copy(subscription, &registration, &copy.remote_subscription, &form, &DeletedMembers::default())
            .await?
        {
            CopyUpdate::Taken => Ok(()),
            CopyUpdate::NotAdvertised => self.place_again(subscription, &registration, copy).await,
            CopyUpdate::Refused(failure) => {
                warn!(subscription = %subscription.id, registration = %registration.id, %failure, "the source refused the update to the current receiver; the copy is placed again");
                self.place_again(subscription, &registration, copy).await
            }
            CopyUpdate::Withheld(failure) => {
                warn!(subscription = %subscription.id, registration = %registration.id, %failure, "the update to the current receiver got no verdict from the source; it is sent again later");
                self.redirects
                    .postpone_redirect(&subscription.id, &RedirectTarget::Copy(copy.clone()), REDIRECT_RETRY)
                    .await
            }
        }
    }

    /// Withdraw `copy` of `subscription` from `registration`'s source and place
    /// it again, as for a registration found matching with no copy there. The
    /// copy placed names the current receiver, under an identifier minted for
    /// it.
    async fn place_again(
        &self,
        subscription: &Subscription,
        registration: &ContextSourceRegistration,
        copy: &ForwardedCopy,
    ) -> Result<(), SubscriptionForwardingError> {
        let withdrawal = ForwardDecision::Delete {
            remote: copy.remote_subscription.clone(),
        };
        self.apply_forward_decision(subscription, registration, withdrawal, &DeletedMembers::default())
            .await?;
        let placement = decide(CsrTriggerReason::NewlyMatching, registration.access.mode.unwrap_or_default(), None);
        self.apply_forward_decision(subscription, registration, placement, &DeletedMembers::default())
            .await
    }
}
