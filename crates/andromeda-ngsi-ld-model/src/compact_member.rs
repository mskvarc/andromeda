//! Decoding one member of a compact node into the typed value the model holds
//! for it.
//!
//! Every member the model types -- an identifier, a `DateTime`, a reference, a
//! language map, an `objectList` -- is decoded by that type's own decoding, so
//! a member read from a compact node becomes exactly what the same member of a
//! decoded document becomes. A string member is handed to the decoding as the
//! string it is, which is how a document's text hands it over; any other
//! member as the JSON value it stands for.

use crate::compact_node::{CompactNode, NodeShape};
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use serde::de::{DeserializeOwned, value::BorrowedStrDeserializer};
use serde_json::{Error as JsonError, Value};

/// The string deserializer a string member is decoded from.
type StringMember<'n> = BorrowedStrDeserializer<'n, JsonError>;

/// Decode `node` as a `T`.
///
/// # Errors
///
/// Returns the node's [`CompactNode::Unreadable`] when a member that is no
/// string has no JSON form; the inner result is the decoding's own outcome.
pub(crate) fn decoded<T: DeserializeOwned, N: CompactNode>(node: &N) -> Result<Result<T, JsonError>, N::Unreadable> {
    match node.shape() {
        NodeShape::String(text) => Ok(T::deserialize(StringMember::new(text))),
        NodeShape::Null | NodeShape::Boolean | NodeShape::Number | NodeShape::Array(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => {
            node.to_json().map(|json: Value| deserialize_exact::<T>(&json))
        }
    }
}

/// Decode `node` as an optional `T`: `null` is no value.
///
/// # Errors
///
/// As [`decoded`].
pub(crate) fn decoded_optional<T: DeserializeOwned, N: CompactNode>(node: &N) -> Result<Result<Option<T>, JsonError>, N::Unreadable> {
    match node.shape() {
        NodeShape::Null => Ok(Ok(None)),
        NodeShape::Boolean | NodeShape::Number | NodeShape::String(_) | NodeShape::Array(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => {
            decoded(node).map(|decoded| decoded.map(Some))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{object_list::RelationshipObjectList, one_or_many::OneOrMany, property::UnitCodeRef},
        compact_member::{StringMember, decoded, decoded_optional},
        entity_type::EntityTypeValue,
    };
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, reference::NgsiLdReference, uri::Uri};
    use andromeda_ngsi_ld_number::json_value::deserialize_exact;
    use chrono::{DateTime, Utc};
    use serde::de::DeserializeOwned;
    use serde_json::{Value, json};
    use std::fmt::Debug;

    /// Whether `T` decodes from `text` the same way through the string and
    /// through its JSON value.
    fn decodes_alike<T: DeserializeOwned + Debug + PartialEq>(text: &str) -> bool {
        let through_string = T::deserialize(StringMember::new(text)).ok();
        let through_json = deserialize_exact::<T>(&Value::String(text.to_owned())).ok();
        through_string == through_json
    }

    /// A string member decodes through the string as it does through its JSON
    /// value, valid or not, for every string-typed member the model holds.
    #[test]
    fn a_string_member_decodes_as_its_json_value_does() {
        for text in [
            "urn:ngsi-ld:T:1",
            "not a uri",
            "2024-01-01T00:00:00Z",
            "2024-01-01T01:00:00+01:00",
            "yesterday",
            "CEL",
            "XYZ",
            "Building",
            "bad ref",
            "",
        ] {
            assert!(decodes_alike::<Identifier>(text), "{text}");
            assert!(decodes_alike::<Uri>(text), "{text}");
            assert!(decodes_alike::<NgsiLdReference>(text), "{text}");
            assert!(decodes_alike::<DateTime<Utc>>(text), "{text}");
            assert!(decodes_alike::<OneOrMany<Uri>>(text), "{text}");
            assert!(decodes_alike::<OneOrMany<NgsiLdReference>>(text), "{text}");
            assert!(decodes_alike::<EntityTypeValue>(text), "{text}");
        }
    }

    /// A unit code decodes only from a borrowed string, which a string member
    /// is handed over as.
    #[test]
    fn a_unit_code_member_decodes_from_its_string() {
        let member = json!("CEL");
        assert!(decoded::<UnitCodeRef, Value>(&member).expect("a string is readable").is_ok());
        assert!(decoded::<UnitCodeRef, Value>(&json!("not a code")).expect("a string is readable").is_err());
    }

    #[test]
    fn null_is_no_optional_value_and_no_required_one() {
        let null = Value::Null;
        assert!(matches!(decoded_optional::<Identifier, Value>(&null), Ok(Ok(None))));
        assert!(decoded::<RelationshipObjectList, Value>(&null).expect("null is readable").is_err());
    }
}
