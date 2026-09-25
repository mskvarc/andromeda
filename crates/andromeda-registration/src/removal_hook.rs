//! The distributed side of removing a registration, kept behind a port.
//!
//! A reduced copy of a local subscription placed at a registration's context
//! source (clause 5.8.1.4) is recorded against that registration, and the store
//! removes the record together with the registration. What the removal must
//! tear down at the context source is therefore captured as a
//! [`RegistrationTeardown`] before the registration's row goes, and handed back
//! once it has. Registration policy states those two moments here and stays
//! ignorant of subscriptions, forwarding and transport.

use crate::error::RegistrationError;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use futures::future::BoxFuture;

/// A reduced copy of a local subscription held at a registration's context
/// source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardedSubscriptionCopy {
    /// The local subscription the copy reduces.
    pub local_subscription: Uri,
    /// The identifier the copy carries at the context source.
    pub remote_subscription: Uri,
}

/// What removing one registration must tear down at its context source,
/// captured while the forwarding records still exist.
#[derive(Debug, Clone)]
pub struct RegistrationTeardown {
    /// The registration being removed, which still names the context source
    /// the copies have to be deleted at once its row is gone.
    pub registration: ContextSourceRegistration,
    /// Every copy forwarded to the registration's context source.
    pub forwarded: Vec<ForwardedSubscriptionCopy>,
}

/// Observes registrations being deleted or expired to tear down their
/// distributed counterparts.
pub trait RegistrationRemovalHook: Send + Sync + 'static {
    /// What removing `registration` must tear down, read before its row and
    /// the records that cascade with it are deleted.
    fn teardown_of<'life0, 'life1, 'async_trait>(
        &'life0 self,
        registration: &'life1 ContextSourceRegistration,
    ) -> BoxFuture<'async_trait, Result<RegistrationTeardown, RegistrationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// The registration `teardown` names was removed; tear down what it left
    /// standing at its context source. Every step is attempted and a failure
    /// is the hook's to report: the registration is gone either way.
    fn removed<'life0, 'life1, 'async_trait>(&'life0 self, teardown: &'life1 RegistrationTeardown) -> BoxFuture<'async_trait, ()>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
}
