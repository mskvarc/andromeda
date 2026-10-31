//! The notifications of one subscription a sink received at or after an instant, counted from the notified entities'
//! `notifiedAt` member (clause 5.2.6), so a test of a subscription's end or expiry compares instants, not their text.

use crate::notification_sink::{NotificationError, NotificationSink, ReceivedNotification, SinkError};
use ngsi_ld::{
    model::{DateTime, Lossless},
    subscription::SubscriptionId,
};
use std::fmt::Display;

/// Why the notifications could not be counted.
#[derive(Debug, thiserror::Error)]
pub enum NotifiedSinceError {
    /// The sink could not be read.
    #[error(transparent)]
    Sink(#[from] SinkError),
    /// A received body is not a notification of entities.
    #[error(transparent)]
    Notification(#[from] NotificationError),
}

/// How many of `received` are notifications of `subscription` with a `notifiedAt` not before `instant`.
///
/// # Errors
///
/// Returns [`NotificationError`] when a body is not a notification of entities in the normalized representation.
pub fn count_since(received: &[ReceivedNotification], subscription: &SubscriptionId, instant: &DateTime) -> Result<usize, NotificationError> {
    received.iter().try_fold(0, |count, notification| {
        let parsed = notification.entities(Lossless::Normalized)?;
        let counted = parsed.subscription_id() == subscription && parsed.notified_at().timestamp() >= instant.timestamp();
        Ok(count + usize::from(counted))
    })
}

/// How many notifications endpoint `n` of `sink` received so far for `subscription` with a `notifiedAt` not before
/// `instant`.
///
/// # Errors
///
/// Returns [`NotifiedSinceError`] when the sink cannot be read or a body is not a notification of entities.
pub async fn notified_since(sink: &NotificationSink, n: impl Display, subscription: &SubscriptionId, instant: &DateTime) -> Result<usize, NotifiedSinceError> {
    Ok(count_since(&sink.received(n).await?, subscription, instant)?)
}

#[cfg(test)]
mod tests {
    use crate::{notification_sink::ReceivedNotification, notified_since::count_since};
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::HeaderMap;
    use ngsi_ld::{model::DateTime, subscription::SubscriptionId};

    /// A notification of subscription `subscription` notified at `notified_at`, carrying no entity.
    fn received(subscription: &str, notified_at: &str) -> ReceivedNotification {
        ReceivedNotification {
            headers: HeaderMap::new(),
            body: Bytes::from(format!(
                r#"{{"id":"urn:ngsi-ld:Notification:1","type":"Notification","subscriptionId":"{subscription}","notifiedAt":"{notified_at}","data":[]}}"#
            )),
        }
    }

    #[gtest]
    fn only_notifications_of_the_subscription_at_or_after_the_instant_count() -> Result<()> {
        let wanted: SubscriptionId = "urn:ngsi-ld:Subscription:1".parse()?;
        let instant: DateTime = "2024-01-01T00:00:03.000Z".parse()?;
        let notifications = [
            received("urn:ngsi-ld:Subscription:1", "2024-01-01T00:00:02.999999Z"),
            received("urn:ngsi-ld:Subscription:1", "2024-01-01T00:00:03Z"),
            received("urn:ngsi-ld:Subscription:1", "2024-01-01T00:00:04.5Z"),
            received("urn:ngsi-ld:Subscription:2", "2024-01-01T00:00:05Z"),
        ];
        verify_that!(count_since(&notifications, &wanted, &instant), ok(eq(&2)))
    }

    #[gtest]
    fn a_body_that_is_not_a_notification_is_refused() -> Result<()> {
        let wanted: SubscriptionId = "urn:ngsi-ld:Subscription:1".parse()?;
        let instant: DateTime = "2024-01-01T00:00:03Z".parse()?;
        let notification = ReceivedNotification {
            headers: HeaderMap::new(),
            body: Bytes::from_static(b"{}"),
        };
        verify_that!(count_since(&[notification], &wanted, &instant), err(anything()))
    }
}
