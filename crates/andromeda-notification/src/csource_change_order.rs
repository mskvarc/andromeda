//! The order in which a tenant's context source registrations and context
//! source registration subscriptions were written (ETSI GS CIM 009 clauses
//! 5.11.2.4 and 5.11.3.4).
//!
//! Each such write draws a [`CsourceChangeRevision`], and revisions order the
//! writes as they committed, whichever broker made them. A broker reaches a
//! change only after later writes may already have committed, so reading the
//! store as it stands would match a registration change against a
//! subscription the change never met, and notify a subscription's write with
//! the state a later write left. The revisions are what lets each change be
//! evaluated against the state it found instead.

use crate::error::NotificationError;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::csource_change_revision::CsourceChangeRevision;
use futures::future::BoxFuture;
use std::collections::HashSet;

/// Whether a claim on a subscription's latest write found it still owed its
/// notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatestWriteClaim {
    /// The write that drew this revision is the latest, and this claim is the
    /// one that notifies it.
    Claimed(CsourceChangeRevision),
    /// The latest write was already claimed, or the subscription is gone.
    Settled,
}

/// Reading and claiming the change revisions of context source
/// subscriptions.
pub trait CsourceChangeOrder: Send + Sync + 'static {
    /// The context source subscriptions whose latest write drew a revision
    /// after `revision`.
    ///
    /// A registration change that drew `revision` found each of them as it
    /// stood before that write; the write's own notification reports the
    /// change, having read the registrations after both committed.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Persistence`] when the store cannot be
    /// read.
    fn written_after(&self, revision: CsourceChangeRevision) -> BoxFuture<'_, Result<HashSet<Identifier>, NotificationError>>;

    /// Claim the notification `subscription`'s latest write owes, unless a
    /// broker already claimed it.
    ///
    /// One write is owed one notification (clauses 5.11.2.4 and 5.11.3.4);
    /// the claim is what keeps an earlier write's change, reached after a
    /// later write committed, from notifying the later state a second time.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Persistence`] when the store cannot be
    /// written.
    fn claim_latest_write<'a>(&'a self, subscription: &'a Identifier) -> BoxFuture<'a, Result<LatestWriteClaim, NotificationError>>;

    /// The revision `subscription`'s latest write drew, `None` once the
    /// subscription is gone.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Persistence`] when the store cannot be
    /// read.
    fn latest_write<'a>(&'a self, subscription: &'a Identifier) -> BoxFuture<'a, Result<Option<CsourceChangeRevision>, NotificationError>>;
}
