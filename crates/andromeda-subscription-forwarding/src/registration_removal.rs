//! Tearing down the reduced copies held at a removed registration's context
//! source (clauses 5.8.1.4 and 5.9.4).
//!
//! The store removes a registration's forwarding records together with the
//! registration, so the copies are captured before the removal and deleted at
//! the context source after it, from the registration the capture kept. The
//! asynchronous `noLongerMatching` notification the removal also triggers then
//! finds no record left and does nothing.

use crate::{error::SubscriptionForwardingError, service::SubscriptionForwardingService};
use andromeda_federation::subscription::{SubscriptionForward, SubscriptionForwardAdmission};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_registration::{
    error::RegistrationError,
    removal_hook::{RegistrationRemovalHook, RegistrationTeardown},
};
use futures::future::BoxFuture;
use tracing::warn;

impl RegistrationRemovalHook for SubscriptionForwardingService {
    fn teardown_of<'life0, 'life1, 'async_trait>(
        &'life0 self,
        registration: &'life1 ContextSourceRegistration,
    ) -> BoxFuture<'async_trait, Result<RegistrationTeardown, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let forwarded = self
                .mappings
                .forwarded_for_registration(&registration.id)
                .await
                .map_err(|error: SubscriptionForwardingError| RegistrationError::persistence(error))?;
            Ok(RegistrationTeardown {
                // clone: the teardown outlives the registration row it names,
                // which is what reaches the context source once the row is gone
                registration: registration.clone(),
                forwarded,
            })
        })
    }

    fn removed<'life0, 'life1, 'async_trait>(&'life0 self, teardown: &'life1 RegistrationTeardown) -> BoxFuture<'async_trait, ()>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let registration = &teardown.registration;
            for copy in &teardown.forwarded {
                let forwarded = self
                    .federation
                    .forward_subscription(
                        registration,
                        SubscriptionForward::Delete {
                            remote_id: &copy.remote_subscription,
                        },
                    )
                    .await;
                if forwarded.admission == SubscriptionForwardAdmission::Forwarded && !forwarded.succeeded() {
                    warn!(subscription = %copy.local_subscription, registration = %registration.id, "forwarded subscription copy could not be deleted at the removed registration's source");
                }
                if let Err(error) = self.router.unregister(&copy.remote_subscription).await {
                    warn!(subscription = %copy.local_subscription, remote = %copy.remote_subscription, %error, "forwarded subscription copy route could not be removed");
                }
            }
        })
    }
}
