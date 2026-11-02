//! Why notifying one subscription of one committed change failed, and the
//! failed attempts of one change counted by whether a retry may succeed.
//!
//! The failure keeps its cause up to the delivery's stored diagnostic, so what
//! is recorded against a queued delivery says what the log said, not only how
//! many attempts failed.

use andromeda_delivery::{endpoint::error::EndpointParseError, endpoint_failure::EndpointFailure, notification_body::NotificationWriteError};
use andromeda_entity::error::EntityError;
use andromeda_subscription::error::SubscriptionError;
use std::num::NonZeroUsize;
use thiserror::Error;

/// Why one subscription was not notified of a change.
#[derive(Debug, Error)]
pub enum NotificationAttemptFailure {
    /// The endpoint refused the notification or was not reached.
    #[error(transparent)]
    Endpoint(EndpointFailure),
    /// The subscription's `jsonldContext` could not be loaded (clause 5.5.6).
    #[error("the jsonldContext of the subscription could not be loaded")]
    ContextUnloadable,
    /// The subscription's notification slot could not be claimed.
    #[error("the notification slot could not be claimed: {0}")]
    SlotUnclaimed(#[source] SubscriptionError),
    /// The change left nothing to notify: the entity no longer exists, or a
    /// deletion carries neither the removed state nor a type.
    #[error("the change left no entity state to notify")]
    NothingToNotify,
    /// The endpoint the subscription names is not one a notification can be
    /// delivered to.
    #[error("the endpoint cannot be delivered to: {0}")]
    EndpointUndeliverable(#[source] EndpointParseError),
    /// The entity rendered as the subscription asks yields no notification
    /// data.
    #[error("the subscription produced no notification data")]
    NoData,
    /// The notification could not be written.
    #[error("the notification could not be written: {0}")]
    Unwritten(#[source] NotificationWriteError),
    /// The entities a subscription filter links to (clause 4.9) could not be
    /// read, so whether the change matches is undecided.
    #[error("the entities a subscription filter links to could not be read: {0}")]
    LinkedEntitiesUnread(#[source] EntityError),
}

impl NotificationAttemptFailure {
    /// Whether a later attempt may succeed.
    ///
    /// A failed read or claim may answer the next attempt; a notification
    /// that cannot be built or addressed fails the same way every time.
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        match self {
            NotificationAttemptFailure::Endpoint(failure) => failure.is_retryable(),
            NotificationAttemptFailure::ContextUnloadable
            | NotificationAttemptFailure::SlotUnclaimed(_)
            | NotificationAttemptFailure::LinkedEntitiesUnread(_) => true,
            NotificationAttemptFailure::NothingToNotify
            | NotificationAttemptFailure::EndpointUndeliverable(_)
            | NotificationAttemptFailure::NoData
            | NotificationAttemptFailure::Unwritten(_) => false,
        }
    }
}

/// The failed attempts of one change that share whether a retry may succeed.
#[derive(Debug)]
pub struct FailedAttempts {
    /// How many attempts failed.
    pub attempts: NonZeroUsize,
    /// Why the first of them failed.
    pub first: NotificationAttemptFailure,
}

impl FailedAttempts {
    /// One failed attempt.
    #[must_use]
    pub const fn new(first: NotificationAttemptFailure) -> FailedAttempts {
        FailedAttempts {
            attempts: NonZeroUsize::MIN,
            first,
        }
    }

    /// Count `failure` into `tally`, keeping the first failure's cause.
    pub fn count(tally: &mut Option<FailedAttempts>, failure: NotificationAttemptFailure) {
        match tally {
            Some(failed) => failed.attempts = failed.attempts.saturating_add(1),
            None => *tally = Some(FailedAttempts::new(failure)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::processor::attempt_failure::{FailedAttempts, NotificationAttemptFailure};
    use andromeda_delivery::{endpoint_failure::EndpointFailure, outcome::EndpointRejection};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_tally_keeps_the_first_cause_and_counts_every_attempt() {
        let mut tally = None;
        FailedAttempts::count(&mut tally, NotificationAttemptFailure::NoData);
        FailedAttempts::count(&mut tally, NotificationAttemptFailure::NothingToNotify);
        let failed = tally.map(|failed| (failed.attempts.get(), failed.first.to_string()));
        assert_eq!(failed, Some((2, "the subscription produced no notification data".to_owned())));
    }

    #[test]
    fn an_endpoint_failure_reads_as_the_endpoint_failure_and_keeps_its_retryability() {
        let rejected = NotificationAttemptFailure::Endpoint(EndpointFailure::Rejected {
            reason: EndpointRejection::HttpStatus { status: 400 },
            retryable: false,
        });
        assert_eq!(rejected.to_string(), "the endpoint answered HTTP 400");
        assert!(!rejected.is_retryable());
    }
}
