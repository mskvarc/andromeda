//! Why notifying one subscription of one committed change failed, and the
//! failed attempts of one change counted by what they say about a retry.
//!
//! The failure keeps its cause up to the delivery's stored diagnostic, so what
//! is recorded against a queued delivery says what the log said, not only how
//! many attempts failed.

use andromeda_delivery::{
    endpoint::error::EndpointParseError,
    endpoint_failure::EndpointFailure,
    failure_class::FailureClass,
    notification_body::NotificationWriteError,
};
use andromeda_entity::error::EntityError;
use andromeda_subscription::error::SubscriptionError;
use std::{num::NonZeroUsize, time::Duration};
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
    /// What the failure says about a later attempt.
    ///
    /// An endpoint failure keeps the class of its answer. A context document,
    /// store or linked entity that could not be read may answer the next
    /// attempt once it recovers, as an unavailable endpoint may; a
    /// notification that cannot be built or addressed fails the same way every
    /// time.
    #[must_use]
    pub const fn class(&self) -> FailureClass {
        match self {
            NotificationAttemptFailure::Endpoint(failure) => failure.class(),
            NotificationAttemptFailure::ContextUnloadable
            | NotificationAttemptFailure::SlotUnclaimed(_)
            | NotificationAttemptFailure::LinkedEntitiesUnread(_) => FailureClass::Unavailable,
            NotificationAttemptFailure::NothingToNotify
            | NotificationAttemptFailure::EndpointUndeliverable(_)
            | NotificationAttemptFailure::NoData
            | NotificationAttemptFailure::Unwritten(_) => FailureClass::Permanent,
        }
    }

    /// The wait the endpoint asked for before the next attempt (RFC 9110
    /// section 10.2.3 `Retry-After`), `None` when it asked for none or the
    /// failure is not an endpoint's.
    #[must_use]
    pub const fn retry_after(&self) -> Option<Duration> {
        match self {
            NotificationAttemptFailure::Endpoint(failure) => failure.retry_after(),
            NotificationAttemptFailure::ContextUnloadable
            | NotificationAttemptFailure::SlotUnclaimed(_)
            | NotificationAttemptFailure::NothingToNotify
            | NotificationAttemptFailure::EndpointUndeliverable(_)
            | NotificationAttemptFailure::NoData
            | NotificationAttemptFailure::Unwritten(_)
            | NotificationAttemptFailure::LinkedEntitiesUnread(_) => None,
        }
    }
}

/// The failed attempts of one change that share a [`FailureClass`].
#[derive(Debug)]
pub struct FailedAttempts {
    /// How many attempts failed.
    pub attempts: NonZeroUsize,
    /// Why the first of them failed.
    pub first: NotificationAttemptFailure,
    /// The longest wait an endpoint asked for before the next attempt, `None`
    /// when none asked.
    pub retry_after: Option<Duration>,
}

impl FailedAttempts {
    /// One failed attempt.
    #[must_use]
    pub const fn new(first: NotificationAttemptFailure) -> FailedAttempts {
        FailedAttempts {
            attempts: NonZeroUsize::MIN,
            retry_after: first.retry_after(),
            first,
        }
    }

    /// Count `failure` into `tally`, keeping the first failure's cause and
    /// the longest wait any of them asked for.
    pub fn count(tally: &mut Option<FailedAttempts>, failure: NotificationAttemptFailure) {
        match tally {
            Some(failed) => {
                failed.attempts = failed.attempts.saturating_add(1);
                failed.retry_after = failed.retry_after.max(failure.retry_after());
            }
            None => *tally = Some(FailedAttempts::new(failure)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::processor::attempt_failure::{FailedAttempts, NotificationAttemptFailure};
    use andromeda_delivery::{endpoint_failure::EndpointFailure, failure_class::FailureClass, outcome::EndpointRejection};
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    /// An endpoint's 503 asking for `retry_after`.
    fn unavailable(retry_after: Option<Duration>) -> NotificationAttemptFailure {
        NotificationAttemptFailure::Endpoint(EndpointFailure::Rejected {
            reason: EndpointRejection::HttpStatus { status: 503, retry_after },
            class: FailureClass::Unavailable,
        })
    }

    #[test]
    fn a_tally_keeps_the_first_cause_and_counts_every_attempt() {
        let mut tally = None;
        FailedAttempts::count(&mut tally, NotificationAttemptFailure::NoData);
        FailedAttempts::count(&mut tally, NotificationAttemptFailure::NothingToNotify);
        let failed = tally.map(|failed| (failed.attempts.get(), failed.first.to_string()));
        assert_eq!(failed, Some((2, "the subscription produced no notification data".to_owned())));
    }

    #[test]
    fn an_endpoint_failure_reads_as_the_endpoint_failure_and_keeps_its_class() {
        let rejected = NotificationAttemptFailure::Endpoint(EndpointFailure::Rejected {
            reason: EndpointRejection::HttpStatus {
                status: 500,
                retry_after: None,
            },
            class: FailureClass::ServerError,
        });
        assert_eq!(rejected.to_string(), "the endpoint answered HTTP 500");
        assert_eq!(rejected.class(), FailureClass::ServerError);
    }

    #[test]
    fn an_endpoint_failure_keeps_the_wait_its_endpoint_asked_for_and_any_other_failure_asks_for_none() {
        assert_eq!(unavailable(Some(Duration::from_secs(7))).retry_after(), Some(Duration::from_secs(7)));
        assert_eq!(unavailable(None).retry_after(), None);
        assert_eq!(NotificationAttemptFailure::ContextUnloadable.retry_after(), None);
    }

    #[test]
    fn a_tally_keeps_the_longest_wait_any_of_its_endpoints_asked_for() {
        let mut tally = None;
        for retry_after in [None, Some(2), Some(5), Some(3)] {
            FailedAttempts::count(&mut tally, unavailable(retry_after.map(Duration::from_secs)));
        }
        assert_eq!(tally.and_then(|failed| failed.retry_after), Some(Duration::from_secs(5)));
        let mut asked_by_none = None;
        FailedAttempts::count(&mut asked_by_none, unavailable(None));
        FailedAttempts::count(&mut asked_by_none, NotificationAttemptFailure::ContextUnloadable);
        assert_eq!(asked_by_none.and_then(|failed| failed.retry_after), None);
    }

    #[test]
    fn a_read_that_may_answer_later_is_unavailability_and_an_unbuildable_notification_permanent() {
        assert_eq!(NotificationAttemptFailure::ContextUnloadable.class(), FailureClass::Unavailable);
        assert_eq!(NotificationAttemptFailure::NoData.class(), FailureClass::Permanent);
        assert_eq!(NotificationAttemptFailure::NothingToNotify.class(), FailureClass::Permanent);
    }
}
