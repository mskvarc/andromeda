//! What a Server-Sent Events client receives.

use crate::stream::registry::{StreamKind, StreamPayload};

/// A single SSE notification event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseNotificationEvent {
    /// The notification identifier, used as the SSE `id:` field so a client
    /// reconnecting with `Last-Event-ID` names something meaningful.
    pub event_id: String,
    /// The serialised notification, used as the SSE `data:` field.
    pub data: String,
}

impl StreamPayload for SseNotificationEvent {
    const KIND: StreamKind = StreamKind::ServerSentEvents;
}

#[cfg(test)]
mod tests {
    use crate::stream::registry::registry_conformance_tests;

    registry_conformance_tests!(
        conformance,
        crate::stream::sse::SseNotificationEvent,
        crate::stream::sse::SseNotificationEvent {
            event_id: "urn:ngsi-ld:Notification:1".to_owned(),
            data: r#"{"id":"urn:ngsi-ld:Notification:1"}"#.to_owned(),
        },
        "SSE"
    );
}
