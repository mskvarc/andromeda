//! The payload a forwarded write is built from, serialised only once a
//! registration needs it.
//!
//! A write handler hands the forwarding layer the payload itself rather than
//! its serialization. Registration matching (clause 5.12) needs only the
//! entity identifier and type, so a write no registration covers -- the common
//! case -- never serialises its payload at all.

use serde::Serialize;
use serde_json::Value;

/// A write payload a forwarded request carries.
pub trait ForwardedBody: Sync {
    /// The payload as the JSON bytes a single-entity forward narrows and sends.
    ///
    /// # Errors
    ///
    /// Returns the serialization failure of the payload.
    fn to_json_bytes(&self) -> Result<Vec<u8>, serde_json::Error>;

    /// The payload as the JSON value a batch forward narrows per registration.
    ///
    /// # Errors
    ///
    /// Returns the serialization failure of the payload.
    fn to_json_value(&self) -> Result<Value, serde_json::Error>;
}

impl<T: Serialize + Sync> ForwardedBody for T {
    fn to_json_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    fn to_json_value(&self) -> Result<Value, serde_json::Error> {
        serde_json::to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use crate::forwarded_body::ForwardedBody;
    use serde_json::json;

    #[test]
    fn a_json_document_forwards_as_its_own_serialization() {
        let document = json!({"id": "urn:ngsi-ld:Vehicle:1", "speed": {"type": "Property", "value": 1}});
        let body: &dyn ForwardedBody = &document;
        pretty_assertions::assert_eq!(
            body.to_json_bytes().expect("serialisable"),
            serde_json::to_vec(&document).expect("serialisable")
        );
        pretty_assertions::assert_eq!(body.to_json_value().expect("serialisable"), document);
    }
}
