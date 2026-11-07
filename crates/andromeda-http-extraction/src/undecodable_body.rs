//! A request body that does not decode into the type its operation reads it as.
//!
//! A serde failure says what was wrong with a value, never which member held
//! it: an Entity whose `id` is one octet longer than an identifier may be is
//! refused with "an identifier of 1025 bytes exceeds the 1024 bytes an
//! identifier may occupy", leaving the client to find the member itself. A
//! body that failed to decode is therefore decoded once more, on that failure
//! path alone, by a deserializer that tracks the member it is in, so the
//! refusal names it. The second decoding reads the text itself, never a JSON
//! tree built from it, through the same reading the first one read
//! it through; the member is tracked beneath them, so the refusal names it as
//! the client wrote it.

use andromeda_ngsi_ld_representation::{aliased_deserializer::AliasedDeserializer, compact_node::context_reading::ContextReading};
use andromeda_ngsi_ld_vocabulary::keyword;
use serde::de::DeserializeOwned;
use serde_json::{Deserializer as JsonReader, value::RawValue};
use serde_path_to_error::{Deserializer as PathTracker, Path, Track};
use std::collections::BTreeMap;
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
    /// The failure decoding the admitted body `text` as a `T` through
    /// `reading` ended in with `source`, naming the member it failed at.
    ///
    /// Called only once a decoding has failed. The retracing decode reads the
    /// text the failed one read, through the same reading, without its
    /// top-level `@context` member, which no decoding reads, so it fails at the
    /// same member.
    #[must_use]
    pub fn of_text<T: DeserializeOwned>(text: &[u8], reading: ContextReading<'_>, source: serde_json::Error) -> UndecodableBody {
        let context_free = without_context(text);
        let mut reader = JsonReader::from_slice(context_free.as_deref().unwrap_or(text));
        let mut track = Track::new();
        let retraced = T::deserialize(AliasedDeserializer::new(PathTracker::new(&mut reader, &mut track), reading));
        let member = track.path();
        match retraced {
            Err(_) if member.iter().next().is_some() => UndecodableBody::Member { member, source },
            Err(_) | Ok(_) => UndecodableBody::Document { source },
        }
    }
}

/// The object `text` is, written again without its top-level `@context`
/// member, when it has one.
///
/// Its other members are carried as the slices of text they are, so nothing of
/// the body is built; their order is not kept, which no failing path depends on.
fn without_context(text: &[u8]) -> Option<Vec<u8>> {
    let mut members: BTreeMap<String, &RawValue> = serde_json::from_slice(text).ok()?;
    members.remove(keyword::CONTEXT)?;
    serde_json::to_vec(&members).ok()
}

#[cfg(test)]
mod tests {
    use crate::undecodable_body::UndecodableBody;
    use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
    use serde::de::DeserializeOwned;
    use std::{collections::HashMap, fmt::Debug};

    /// Decode `text` as a `T` and classify the failure.
    fn undecodable<T: DeserializeOwned + Debug>(text: &str) -> UndecodableBody {
        let source = serde_json::from_str::<T>(text).expect_err("the fixture must not decode");
        UndecodableBody::of_text::<T>(text.as_bytes(), ContextReading::keyword_spellings(), source)
    }

    #[test]
    fn a_failing_top_level_member_is_named() {
        let error = undecodable::<HashMap<String, u16>>(r#"{"id": "not a number"}"#);
        assert!(
            matches!(&error, UndecodableBody::Member { member, .. } if member.to_string() == "id"),
            "{error:?}"
        );
        assert!(error.to_string().starts_with("the request body member 'id' does not decode: "), "{error}");
    }

    #[test]
    fn a_failing_nested_member_is_named_by_its_path() {
        let error = undecodable::<HashMap<String, HashMap<String, u16>>>(r#"{"endpoint": {"port": "x"}}"#);
        assert!(
            matches!(&error, UndecodableBody::Member { member, .. } if member.to_string() == "endpoint.port"),
            "{error:?}"
        );
    }

    #[test]
    fn a_document_of_the_wrong_shape_names_no_member() {
        let error = undecodable::<HashMap<String, u16>>("[1, 2]");
        assert!(matches!(error, UndecodableBody::Document { .. }), "{error:?}");
        assert!(error.to_string().starts_with("the request body does not decode: "), "{error}");
    }

    /// The `@context` a body names is no member of the resource, so a failure
    /// elsewhere is named even when the type refuses unknown members.
    #[test]
    fn a_failing_member_is_named_beside_the_context_the_body_carries() {
        #[derive(Debug, serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Strict {
            #[serde(rename = "id")]
            _id: u16,
        }

        let text = r#"{"@context": "https://example.org/a", "id": "x"}"#;
        let source = serde_json::from_str::<u16>("\"x\"").expect_err("a string is no number");
        let error = UndecodableBody::of_text::<Strict>(text.as_bytes(), ContextReading::keyword_spellings(), source);
        assert!(
            matches!(&error, UndecodableBody::Member { member, .. } if member.to_string() == "id"),
            "{error:?}"
        );
    }

    /// The member is named as the client wrote it, though it was read as the
    /// core term its spelling stands for.
    #[test]
    fn a_failing_member_is_named_as_written() {
        #[derive(Debug, serde::Deserialize)]
        struct Typed {
            #[serde(rename = "id")]
            _id: u16,
        }

        let text = r#"{"@id": "x"}"#;
        let source = serde_json::from_str::<u16>("\"x\"").expect_err("a string is no number");
        let error = UndecodableBody::of_text::<Typed>(text.as_bytes(), ContextReading::keyword_spellings(), source);
        assert!(
            matches!(&error, UndecodableBody::Member { member, .. } if member.to_string() == "@id"),
            "{error:?}"
        );
    }
}
