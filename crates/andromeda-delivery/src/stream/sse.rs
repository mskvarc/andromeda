//! What a Server-Sent Events client receives.

use crate::stream::registry::{StreamKind, StreamPayload};
use andromeda_ngsi_ld_lexicon::uri::Uri;

/// A single SSE notification event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseNotificationEvent {
    /// The notification identifier, used as the SSE `id:` field so a client
    /// reconnecting with `Last-Event-ID` names something meaningful.
    ///
    /// An NGSI-LD identifier — the notification's own `id`, or the subscription
    /// id when the payload carries none — so it is a [`Uri`], not free text.
    pub event_id: Uri,
    /// The serialised notification, used as the SSE `data:` field.
    pub data: String,
}

impl StreamPayload for SseNotificationEvent {
    const KIND: StreamKind = StreamKind::ServerSentEvents;
}

#[cfg(test)]
mod tests {
    use crate::stream::registry::registry_conformance_tests;

    // The event's `event_id` is built with `Uri::from_str`, which the macro
    // resolves through its own module imports rather than this one.
    registry_conformance_tests!(
        conformance,
        super::super::SseNotificationEvent,
        super::super::SseNotificationEvent {
            event_id: Uri::from_str("urn:ngsi-ld:Notification:1").expect("a valid notification id"),
            data: r#"{"id":"urn:ngsi-ld:Notification:1"}"#.to_owned(),
        },
        "SSE"
    );
}
