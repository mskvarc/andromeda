//! Which companions and copies still name a receiver this broker no longer
//! serves (clause 5.8.1.4).
//!
//! A companion and every copy notify the receiver below this broker's
//! advertised origin. That receiver changes when the operator advertises
//! another origin, or when a release moves the receiver's path. A companion or
//! copy placed before the change keeps notifying the receiver it was placed
//! with until it is updated, so the ledger records each one that is owed the
//! current receiver, with the instant its update is next due. A tenant adopts
//! the current receiver before it serves a request, and when that receiver
//! differs from the one the ledger last adopted, every companion and every
//! settled copy is owed it from then on. A copy whose claim is still open is
//! left out, since its placement sends the current receiver and its
//! settlement says whether the source holds that send.

use crate::{error::SubscriptionForwardingError, receiver::ReceiverEndpoint};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_subscription::forwarding_hook::ForwardedCopy;
use futures::future::BoxFuture;
use std::time::Duration;

/// What of a local subscription is owed the current receiver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedirectTarget {
    /// Its companion context source registration subscription.
    Companion,
    /// Its copy at a registration's context source.
    Copy(ForwardedCopy),
}

/// A companion or copy owed the current receiver, whose update has fallen due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwedRedirect {
    /// The local subscription the companion mirrors or the copy reduces.
    pub local: Identifier,
    /// The companion or the copy.
    pub target: RedirectTarget,
}

/// The ledger of the companions and copies owed the current receiver.
pub trait RedirectLedger: Send + Sync + 'static {
    /// Adopt `receiver` as the one this tenant's companions and copies name.
    ///
    /// When it differs from the receiver adopted last, or none was, every
    /// companion and every copy whose claim is settled is owed it from now.
    /// Adopting the receiver adopted last changes nothing.
    fn adopt_receiver<'life0, 'life1, 'async_trait>(
        &'life0 self,
        receiver: &'life1 ReceiverEndpoint,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Up to `limit` companions and up to `limit` copies whose update to the
    /// current receiver has fallen due, the longest overdue of each first.
    fn overdue_redirects<'life0, 'async_trait>(&'life0 self, limit: u32) -> BoxFuture<'async_trait, Result<Vec<OwedRedirect>, SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// Record that `target` of `local` names the current receiver. A target
    /// that owed nothing is left as it is.
    fn redirected<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        local: &'life1 Identifier,
        target: &'life2 RedirectTarget,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Make the update of `target` of `local` due again `after` from now. A
    /// target that owes nothing is left as it is.
    fn postpone_redirect<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        local: &'life1 Identifier,
        target: &'life2 RedirectTarget,
        after: Duration,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionForwardingError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
}
