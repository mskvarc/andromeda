//! What a WebSocket client receives.

use crate::stream::registry::{StreamKind, StreamPayload};

/// A single WebSocket notification message, sent as a text frame.
///
/// Unlike SSE there is no protocol-level event identifier: the notification's
/// own `id` member is the only one, and it is already inside `data`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsNotificationMessage {
    /// The serialised notification.
    pub data: String,
}

impl StreamPayload for WsNotificationMessage {
    const KIND: StreamKind = StreamKind::WebSocket;
}

#[cfg(test)]
mod tests {
    use crate::stream::registry::registry_conformance_tests;

    registry_conformance_tests!(
        conformance,
        crate::stream::ws::WsNotificationMessage,
        crate::stream::ws::WsNotificationMessage {
            data: r#"{"id":"urn:ngsi-ld:Notification:1"}"#.to_owned(),
        },
        "WebSocket"
    );
}
