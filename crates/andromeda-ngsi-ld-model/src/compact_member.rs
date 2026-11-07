//! Decoding one member of a compact node into the typed value the model holds
//! for it.
//!
//! Every member the model types -- an identifier, a `DateTime`, a reference, a
//! language map, an `objectList` -- is decoded by that type's own decoding, so
//! a member read from a compact node becomes exactly what the same member of a
//! decoded document becomes.

use andromeda_ngsi_ld_representation::compact_node::CompactNode;
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

#[cfg(test)]
mod tests {
    use crate::{attribute::object_list::RelationshipObjectList, compact_member::decoded};
    use andromeda_json_literal::piece::JsonPiece;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use cefact_units::UnitCode;
    use serde_json::{Value, json, value::RawValue};

    /// The text of `value` as raw JSON, the way a request hands it over.
    fn written(value: &Value) -> Box<RawValue> {
        RawValue::from_string(value.to_string()).expect("the fixture is JSON")
    }

    #[test]
    fn a_unit_code_member_decodes_from_its_string() {
        let member = written(&json!("CEL"));
        let invalid = written(&json!("not a code"));
        assert!(decoded::<UnitCode, _>(JsonPiece::new(&member)).expect("a string is readable").is_ok());
        assert!(decoded::<UnitCode, _>(JsonPiece::new(&invalid)).expect("a string is readable").is_err());
    }

    #[test]
    fn null_is_no_value() {
        let null = written(&Value::Null);
        assert!(decoded::<Identifier, _>(JsonPiece::new(&null)).expect("null is readable").is_err());
        assert!(decoded::<RelationshipObjectList, _>(JsonPiece::new(&null)).expect("null is readable").is_err());
    }
}
