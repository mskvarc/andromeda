//! The clause 5.8.6 outcome a delivery result records on its subscription.

use andromeda_delivery::dispatch::DeliveryResult;
use andromeda_subscription::notification_slot::NotificationOutcome;

/// The outcome `delivery` records on the subscription.
///
/// Clause 5.8.6 stamps `lastSuccess` for a 200 OK response and `lastFailure`
/// for any other, so a failure of every class is recorded alike.
pub(crate) const fn notification_outcome(delivery: &DeliveryResult) -> NotificationOutcome {
    match delivery {
        DeliveryResult::Delivered => NotificationOutcome::Success,
        DeliveryResult::Failed(_) => NotificationOutcome::Failure,
    }
}

#[cfg(test)]
mod tests {
    use crate::notification_outcome::notification_outcome;
    use andromeda_delivery::{dispatch::DeliveryResult, endpoint_failure::EndpointFailure, failure_class::FailureClass, outcome::EndpointRejection};
    use andromeda_subscription::notification_slot::NotificationOutcome;
    use pretty_assertions::assert_eq;

    #[test]
    fn only_an_accepted_notification_is_recorded_as_a_success() {
        assert_eq!(notification_outcome(&DeliveryResult::Delivered), NotificationOutcome::Success);
        for (status, class) in [
            (503, FailureClass::Unavailable),
            (500, FailureClass::ServerError),
            (400, FailureClass::Permanent),
        ] {
            let failed = DeliveryResult::Failed(EndpointFailure::Rejected {
                reason: EndpointRejection::HttpStatus { status },
                class,
            });
            assert_eq!(notification_outcome(&failed), NotificationOutcome::Failure);
        }
    }
}
