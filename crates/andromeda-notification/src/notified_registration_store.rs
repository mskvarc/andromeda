//! The store of the context source registrations each context source
//! registration subscription was notified of as matching (ETSI GS CIM 009
//! clauses 5.3.3 and 5.11.7).
//!
//! Every operation is one atomic decision the store makes on the caller's
//! behalf: whether a registration was already notified is read and changed in
//! the same step, so two brokers evaluating the same change against the same
//! subscription never both act on it.

use crate::error::NotificationError;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::csource_change_revision::CsourceChangeRevision;
use futures::future::BoxFuture;

/// Whether a release found the registration among those notified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Release {
    /// The registration was notified as matching and no longer is.
    Released,
    /// The registration was not notified as matching.
    Absent,
}

/// Whether a report claim found the `updated` notification of a registration
/// change still owed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateReport {
    /// This claim is the one that sends the notification.
    Claimed,
    /// A claim for this change or a later one already holds it, or the
    /// registration is not recorded for the subscription.
    Settled,
}

/// Recording which registrations each subscription was notified of.
pub trait NotifiedRegistrationStore: Send + Sync + 'static {
    /// Claim the `updated` notification the registration change that drew
    /// `revision` owes `subscription` for `registration`, unless a claim for
    /// that change or a later one already holds it.
    ///
    /// One change is owed one notification (clause 5.11.7); the claim is what
    /// keeps a second evaluation of the same change from sending it again.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Persistence`] when the store cannot be
    /// written.
    fn report_update<'a>(
        &'a self,
        subscription: &'a Identifier,
        registration: &'a Identifier,
        revision: CsourceChangeRevision,
    ) -> BoxFuture<'a, Result<UpdateReport, NotificationError>>;

    /// Give back the claim `revision` holds on the `updated` notification of
    /// `registration`, when that notification was not sent, so the next
    /// evaluation of the change can send it.
    ///
    /// A claim a later change holds is left as it is.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Persistence`] when the store cannot be
    /// written.
    fn unreport_update<'a>(
        &'a self,
        subscription: &'a Identifier,
        registration: &'a Identifier,
        revision: CsourceChangeRevision,
    ) -> BoxFuture<'a, Result<(), NotificationError>>;

    /// Record each of `registrations` as notified to `subscription` unless it
    /// already is, and return those this call recorded.
    ///
    /// A subscription that no longer exists records nothing.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Persistence`] when the store cannot be
    /// written.
    fn claim<'a>(&'a self, subscription: &'a Identifier, registrations: &'a [Identifier]) -> BoxFuture<'a, Result<Vec<Identifier>, NotificationError>>;

    /// Remove `registration` from the registrations notified to
    /// `subscription`, answering whether it was among them.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Persistence`] when the store cannot be
    /// written.
    fn release<'a>(&'a self, subscription: &'a Identifier, registration: &'a Identifier) -> BoxFuture<'a, Result<Release, NotificationError>>;

    /// The subscriptions `registration` was notified to as matching.
    ///
    /// A read, not a decision: it narrows which subscriptions a change of the
    /// registration has to release it from, and each release still decides for
    /// itself.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Persistence`] when the store cannot be
    /// read.
    fn notified_subscriptions<'a>(&'a self, registration: &'a Identifier) -> BoxFuture<'a, Result<Vec<Identifier>, NotificationError>>;

    /// The registrations notified to `subscription` as matching.
    ///
    /// A read, not a decision: it lists the records an update of the
    /// subscription re-examines, and each withdrawal still decides for itself.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Persistence`] when the store cannot be
    /// read.
    fn registrations_notified_to<'a>(&'a self, subscription: &'a Identifier) -> BoxFuture<'a, Result<Vec<Identifier>, NotificationError>>;
}
