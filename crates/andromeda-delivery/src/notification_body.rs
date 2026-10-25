//! A notification as the bytes every transport sends.
//!
//! The notification is written once, by whoever knows what it says, and every
//! transport sends that text: the HTTP body as it is, the MQTT envelope with it
//! as its `body`, the SSE and WebSocket frames as their data. No transport
//! reads the document back, and none writes it a second time.

use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::NotificationIdParseError};
use serde::Serialize;
use serde_json::value::{RawValue, to_raw_value};
use std::sync::Arc;
use thiserror::Error;

/// Why a notification could not be written.
#[derive(Debug, Error)]
pub enum NotificationWriteError {
    /// No notification identifier could be minted.
    #[error("the notification identifier could not be minted: {0}")]
    Identifier(#[from] NotificationIdParseError),
    /// The notification could not be written.
    #[error("the notification could not be written: {0}")]
    Written(#[from] serde_json::Error),
}

/// A written notification and the identifier it names itself by.
#[derive(Debug, Clone)]
pub struct NotificationBody {
    /// The notification's own `id`, which an SSE frame names its event by.
    id: Identifier,
    /// The written notification, shared by every delivery attempt.
    json: Arc<RawValue>,
}

impl NotificationBody {
    /// The notification `document`, written, naming itself `id`.
    ///
    /// The document borrows the identifier it writes, so the body takes its
    /// own copy of it.
    ///
    /// # Errors
    ///
    /// Returns the [`serde_json::Error`] `document` cannot be written with.
    pub fn written<T: Serialize + ?Sized>(id: &Identifier, document: &T) -> Result<NotificationBody, serde_json::Error> {
        // clone: the body owns the identifier the document only borrows
        to_raw_value(document).map(|json| NotificationBody::new(id.clone(), json))
    }

    /// A notification already written as `json`, naming itself `id`.
    #[must_use]
    pub fn new(id: Identifier, json: Box<RawValue>) -> NotificationBody {
        NotificationBody { id, json: Arc::from(json) }
    }

    /// The notification's own identifier.
    #[must_use]
    pub const fn id(&self) -> &Identifier {
        &self.id
    }

    /// The written notification.
    #[must_use]
    pub fn json(&self) -> &RawValue {
        &self.json
    }

    /// The written notification's text.
    #[must_use]
    pub fn text(&self) -> &str {
        self.json.get()
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::notification_body::NotificationBody;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use std::str::FromStr;

    #[test]
    fn a_written_notification_keeps_its_text_and_its_identifier() {
        let id = Identifier::from_str("urn:ngsi-ld:Notification:1").expect("an identifier");
        let document: serde_json::Value = serde_json::from_str(r#"{"id":"urn:ngsi-ld:Notification:1","data":[0.100000000000000005]}"#).expect("JSON");
        let body = NotificationBody::written(&id, &document).expect("written");
        pretty_assertions::assert_eq!(body.id().as_str(), "urn:ngsi-ld:Notification:1");
        pretty_assertions::assert_eq!(body.text(), r#"{"data":[0.100000000000000005],"id":"urn:ngsi-ld:Notification:1"}"#);
    }
}
