//! A notification whose entities a `pick` or `omit` of the subscription may have left without `id` or `type`
//! (clause 4.21), read as projected entities.

use crate::notification_sink::{NotificationError, ReceivedNotification};
use ngsi_ld::{
    model::{Entity, Lossless, Output, Projected},
    subscription::Notification,
};
use std::str;

/// The body of `received` read as a notification of projected entities in normalized or concise representation
/// (clause 5.3.1).
///
/// # Errors
///
/// Returns [`NotificationError`] when the body is not such a notification.
pub fn projected_entities(received: &ReceivedNotification, form: Lossless) -> Result<Notification<Vec<Entity<Output, Projected>>>, NotificationError> {
    Ok(Notification::<Vec<Entity<Output, Projected>>>::parse(str::from_utf8(&received.body)?, form)?)
}

#[cfg(test)]
mod tests {
    use crate::{notification_sink::ReceivedNotification, projected_notification::projected_entities};
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::HeaderMap;
    use ngsi_ld::model::Lossless;

    #[gtest]
    fn entities_without_id_or_type_are_read() -> Result<()> {
        let received = ReceivedNotification {
            headers: HeaderMap::new(),
            body: Bytes::from_static(
                br#"{"id":"urn:ngsi-ld:Notification:1","type":"Notification","subscriptionId":"urn:ngsi-ld:Subscription:1",
                "notifiedAt":"2026-01-01T00:00:00Z","data":[{"id":"urn:ngsi-ld:Building:1","owner":{"type":"Relationship","object":"urn:ngsi-ld:Person:1"}},
                {"name":{"type":"Property","value":"Ada"}}]}"#,
            ),
        };
        let notification = projected_entities(&received, Lossless::Normalized)?;
        let data = notification.data();
        expect_that!(data, len(eq(2)));
        expect_that!(data.first().map(|entity| entity.id.is_some()), some(eq(true)));
        verify_that!(data.get(1).map(|entity| entity.id.is_none()), some(eq(true)))
    }

    #[gtest]
    fn a_body_that_is_not_a_notification_is_refused() -> Result<()> {
        let received = ReceivedNotification {
            headers: HeaderMap::new(),
            body: Bytes::from_static(b"[]"),
        };
        verify_that!(projected_entities(&received, Lossless::Normalized), err(anything()))
    }
}
