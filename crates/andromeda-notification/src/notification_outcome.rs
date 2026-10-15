//! The clause 5.8.6 outcome a delivery result records on its subscription.

use andromeda_delivery::dispatch::DeliveryResult;
use andromeda_subscription::notification_slot::NotificationOutcome;

/// The outcome `delivery` records on the subscription.
///
/// Clause 5.8.6 stamps `lastSuccess` for a 200 OK response and `lastFailure`
/// for any other, so a failure worth retrying is recorded exactly as a
/// terminal one.
pub(crate) const fn notification_outcome(delivery: &DeliveryResult) -> NotificationOutcome {
    match delivery {
        DeliveryResult::Delivered => NotificationOutcome::Success,
        DeliveryResult::RetryableFailure | DeliveryResult::TerminalFailure => NotificationOutcome::Failure,
    }
}

#[cfg(test)]
mod tests {
    use crate::notification_outcome::notification_outcome;
    use andromeda_delivery::dispatch::DeliveryResult;
    use andromeda_subscription::notification_slot::NotificationOutcome;
    use pretty_assertions::assert_eq;

    #[test]
    fn only_an_accepted_notification_is_recorded_as_a_success() {
        assert_eq!(notification_outcome(&DeliveryResult::Delivered), NotificationOutcome::Success);
        assert_eq!(notification_outcome(&DeliveryResult::RetryableFailure), NotificationOutcome::Failure);
        assert_eq!(notification_outcome(&DeliveryResult::TerminalFailure), NotificationOutcome::Failure);
    }
}
