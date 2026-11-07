//! A notification as the bytes every transport sends.
//!
//! The notification is written once, by whoever knows what it says, and every
//! transport sends that text: the HTTP body as it is, the MQTT envelope with it
//! as its `body`. No transport reads the document back, and none writes it a
//! second time.

use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::NotificationIdParseError;
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

/// A written notification, shared by every delivery attempt.
#[derive(Debug, Clone)]
pub struct NotificationBody {
    /// The written notification.
    json: Arc<RawValue>,
}

impl NotificationBody {
    /// The notification `document`, written.
    ///
    /// # Errors
    ///
    /// Returns the [`serde_json::Error`] `document` cannot be written with.
    pub fn written<T: Serialize + ?Sized>(document: &T) -> Result<NotificationBody, serde_json::Error> {
        to_raw_value(document).map(NotificationBody::new)
    }

    /// A notification already written as `json`.
    #[must_use]
    pub fn new(json: Box<RawValue>) -> NotificationBody {
        NotificationBody { json: Arc::from(json) }
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
mod tests {
    use crate::notification_body::NotificationBody;

    #[test]
    fn a_written_notification_keeps_its_text() {
        let document: serde_json::Value = serde_json::from_str(r#"{"id":"urn:ngsi-ld:Notification:1","data":[0.100000000000000005]}"#).expect("JSON");
        let body = NotificationBody::written(&document).expect("written");
        pretty_assertions::assert_eq!(body.text(), r#"{"data":[0.100000000000000005],"id":"urn:ngsi-ld:Notification:1"}"#);
    }
}
