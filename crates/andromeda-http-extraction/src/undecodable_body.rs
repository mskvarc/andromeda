//! A request body that does not decode into the type its operation reads it as.
//!
//! A serde failure says what was wrong with a value, never which member held
//! it: an Entity whose `id` is one octet longer than an identifier may be was
//! refused with "an identifier of 1025 bytes exceeds the 1024 bytes an
//! identifier may occupy", leaving the client to find the member itself. A
//! body that failed to decode is therefore decoded once more, on that failure
//! path alone, by a deserializer that tracks the member it is in, so the
//! refusal names it.

use crate::admitted_document::admitted_document;
use serde::de::DeserializeOwned;
use serde_json::Value;
use serde_path_to_error::Path;
use thiserror::Error;

/// Why a request body does not decode, and where in it the decoding failed.
#[derive(Debug, Error)]
pub enum UndecodableBody {
    /// The document as a whole has the wrong shape.
    #[error("the request body does not decode: {source}")]
    Document {
        /// The reader's failure, with its position in the body.
        #[source]
        source: serde_json::Error,
    },
    /// One member of the document does not decode.
    #[error("the request body member '{member}' does not decode: {source}")]
    Member {
        /// The path of the failing member, such as `id` or `endpoint.uri`.
        member: Path,
        /// The reader's failure, with its position in the body.
        #[source]
        source: serde_json::Error,
    },
}

impl UndecodableBody {
    /// The failure decoding `document` as a `T` ended in with `source`,
    /// naming the member it failed at.
    ///
    /// Called only once a decoding has failed. The retracing decode reads the
    /// same `Value` the failed one read, so it fails at the same member.
    #[must_use]
    pub fn of<T: DeserializeOwned>(document: &Value, source: serde_json::Error) -> UndecodableBody {
        match serde_path_to_error::deserialize::<&Value, T>(document) {
            Err(retraced) if retraced.path().iter().next().is_some() => UndecodableBody::Member {
                member: retraced.path().clone(),
                source,
            },
            Err(_) | Ok(_) => UndecodableBody::Document { source },
        }
    }

    /// The failure decoding the admitted body `text` as a `T` ended in with
    /// `source`, naming the member it failed at.
    ///
    /// Called only once a decoding has failed: the body is parsed into the
    /// tree an admitted body stands for, whose members the retracing decode
    /// reads as the failed decoding read them.
    #[must_use]
    pub fn of_text<T: DeserializeOwned>(text: &[u8], source: serde_json::Error) -> UndecodableBody {
        match admitted_document(text) {
            Ok(document) => UndecodableBody::of::<T>(&document, source),
            Err(_unreadable) => UndecodableBody::Document { source },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::undecodable_body::UndecodableBody;
    use serde::de::DeserializeOwned;
    use serde_json::{Value, json};
    use std::{collections::HashMap, fmt::Debug};

    /// Decode `document` as a `T` and classify the failure.
    fn undecodable<T: DeserializeOwned + Debug>(document: &Value) -> UndecodableBody {
        let source = T::deserialize(document).expect_err("the fixture must not decode");
        UndecodableBody::of::<T>(document, source)
    }

    #[test]
    fn a_failing_top_level_member_is_named() {
        let error = undecodable::<HashMap<String, u16>>(&json!({"id": "not a number"}));
        assert!(
            matches!(&error, UndecodableBody::Member { member, .. } if member.to_string() == "id"),
            "{error:?}"
        );
        assert!(error.to_string().starts_with("the request body member 'id' does not decode: "), "{error}");
    }

    #[test]
    fn a_failing_nested_member_is_named_by_its_path() {
        let error = undecodable::<HashMap<String, HashMap<String, u16>>>(&json!({"endpoint": {"port": "x"}}));
        assert!(
            matches!(&error, UndecodableBody::Member { member, .. } if member.to_string() == "endpoint.port"),
            "{error:?}"
        );
    }

    #[test]
    fn a_document_of_the_wrong_shape_names_no_member() {
        let error = undecodable::<HashMap<String, u16>>(&json!([1, 2]));
        assert!(matches!(error, UndecodableBody::Document { .. }), "{error:?}");
        assert!(error.to_string().starts_with("the request body does not decode: "), "{error}");
    }
}
