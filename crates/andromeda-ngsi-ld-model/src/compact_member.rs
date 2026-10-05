//! Decoding one member of a compact node into the typed value the model holds
//! for it.
//!
//! Every member the model types -- an identifier, a `DateTime`, a reference, a
//! language map, an `objectList` -- is decoded by that type's own decoding, so
//! a member read from a compact node becomes exactly what the same member of a
//! decoded document becomes.

use andromeda_ngsi_ld_representation::compact_node::{CompactNode, NodeShape};
use serde::de::DeserializeOwned;
use serde_json::Error as JsonError;

/// Decode `node` as a `T`.
///
/// # Errors
///
/// Returns the node's [`CompactNode::Unreadable`] when it has no JSON form;
/// the inner result is the decoding's own outcome.
pub(crate) fn decoded<'n, T: DeserializeOwned, N: CompactNode<'n>>(node: N) -> Result<Result<T, JsonError>, N::Unreadable> {
    node.decoded()
}

/// Decode `node` as an optional `T`: `null` is no value.
///
/// # Errors
///
/// As [`decoded`].
pub(crate) fn decoded_optional<'n, T: DeserializeOwned, N: CompactNode<'n>>(node: N) -> Result<Result<Option<T>, JsonError>, N::Unreadable> {
    match node.shape() {
        NodeShape::Null => Ok(Ok(None)),
        NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Array(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => {
            decoded(node).map(|decoded| decoded.map(Some))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute::{object_list::RelationshipObjectList, property::UnitCodeRef},
        compact_member::{decoded, decoded_optional},
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use serde_json::{Value, json};

    /// A unit code decodes only from a borrowed string, which a string member
    /// is handed over as.
    #[test]
    fn a_unit_code_member_decodes_from_its_string() {
        let member = json!("CEL");
        assert!(decoded::<UnitCodeRef, &Value>(&member).expect("a string is readable").is_ok());
        assert!(decoded::<UnitCodeRef, &Value>(&json!("not a code")).expect("a string is readable").is_err());
    }

    #[test]
    fn null_is_no_optional_value_and_no_required_one() {
        let null = Value::Null;
        assert!(matches!(decoded_optional::<Identifier, &Value>(&null), Ok(Ok(None))));
        assert!(decoded::<RelationshipObjectList, &Value>(&null).expect("null is readable").is_err());
    }
}
