//! Tearing down what a local subscription left at the context sources: every
//! forwarded copy and the companion (clause 5.8.5.4).
//!
//! A deletion is torn down from what was captured before the local row went:
//! the store removes the forwarding mappings together with the subscription,
//! so only the remote copies, the companion subscription and the platform
//! routes are left for this side to remove. A withdrawal, which an expiry or a
//! subscription leaving distributed operations calls for, keeps the local row,
//! so it removes each copy's mapping as well.

use crate::{error::SubscriptionForwardingError, service::SubscriptionForwardingService};
use andromeda_federation::subscription::SubscriptionForward;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_subscription::forwarding_hook::{ForwardedCopy, SubscriptionTeardown};
use tracing::{debug, warn};

/// Where the mappings of the copies being torn down stand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CopyMappings {
    /// The local subscription was deleted, and its mappings with it.
    RemovedWithSubscription,
    /// The local subscription still exists, so each copy's mapping is removed
    /// here as the copy is.
    StillRecorded,
}

impl SubscriptionForwardingService {
    /// What tearing `local` down involves, read from the mappings while they
    /// still exist.
    pub(crate) async fn teardown_of(&self, local: &Identifier) -> Result<SubscriptionTeardown, SubscriptionForwardingError> {
        Ok(SubscriptionTeardown {
            companion: self.mappings.companion_of(local).await?,
            forwarded: self.mappings.forwarded_of(local).await?,
        })
    }

    /// Withdraw the companion and every forwarded copy of `local`, whose row
    /// and mappings still exist.
    pub(crate) async fn withdraw(&self, local: &Identifier) -> Result<(), SubscriptionForwardingError> {
        let teardown = self.teardown_of(local).await?;
        self.tear_down(local, &teardown, CopyMappings::StillRecorded).await;
        Ok(())
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
    pub(crate) async fn tear_down(&self, local: &Identifier, teardown: &SubscriptionTeardown, mappings: CopyMappings) {
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
                if let Some(failure) = forwarded.failure() {
                    warn!(subscription = %local, registration = %copy.registration, %failure, "forwarded subscription copy could not be deleted at the source");
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
}
