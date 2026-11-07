//! The scalar members of a stored array value, the values a clause 4.9 `q`
//! equality on it matches.
//!
//! A value that is not one scalar is stored as the JSON of the structured
//! value codec. When that JSON is an array (several values, a LanguageProperty
//! map, several Relationship objects) an equality matches a value the array
//! includes, so each member that is a number, a string or a boolean is one
//! value, and so is the `@value` of a member value node when it is one of
//! those. A string member is declared by the instance's `valueType` when it
//! has one, which declares every primitive member of its value (clause
//! 4.5.2), and by its own `@type` otherwise. One declared as a clause 4.6.3
//! time value, an XSD number or an `xsd:boolean` is that value; a declared
//! number beyond the decimals equals no operand and holds none. Every other
//! member (`null`, an array, a node without such a `@value`) and a value that
//! is not an array hold none.
//!
//! The members are read from the stored text: a member's JSON type is fixed by
//! its first byte, and a number is read at arbitrary precision so it keys as
//! the exact decimal it is.

use crate::stored_json::StoredJson;
use andromeda_ngsi_ld_lexicon::{
    datatype::{declaration::LiteralDeclaration, declared_value::DeclaredValue, primitive_literal::PrimitiveLiteral, recognised::DatatypeError},
    reference::NgsiLdReference,
    time::value::TimeValue,
};
use andromeda_ngsi_ld_number::{JsonNumber, JsonNumberError, declared_number::DeclaredNumber};
use serde::Deserialize;
use serde_json::value::RawValue;
use thiserror::Error;

/// One scalar member of a stored array.
#[derive(Debug, PartialEq)]
pub enum ArrayMember {
    /// A number, at its exact value.
    Number(JsonNumber),
    /// A string its datatype gives no other data type.
    Text(String),
    /// A boolean.
    Boolean(bool),
    /// A value node declared as a time value.
    Time(TimeValue),
}

/// A member value node, of which `@value` and `@type` are read.
#[derive(Deserialize)]
struct ValueNode<'a> {
    /// The node's `@value`, absent on a node that has none.
    #[serde(rename = "@value", borrow)]
    value: Option<&'a RawValue>,
    /// The node's datatype IRI, absent on a node that declares none.
    #[serde(rename = "@type", borrow)]
    datatype: Option<&'a str>,
}

/// A stored array whose members do not read back.
#[derive(Debug, Error)]
pub enum StoredArrayMemberError {
    /// The stored text is not the JSON its codec writes.
    #[error("a stored array value does not read as JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// A member number has no exact decimal value.
    #[error("a stored array member is not a storable number: {0}")]
    Number(#[from] JsonNumberError),
    /// A member is no value of the datatype it is declared as.
    #[error("a stored array member is no value of its declared datatype: {0}")]
    Declared(#[from] DatatypeError),
}

/// The scalar members of `json`, a value beside the `valueType`
/// `value_type`, in stored order; none when it is not an array.
///
/// # Errors
///
/// Returns [`StoredArrayMemberError`] when the stored text does not read back
/// as the JSON its codec writes, or a member is no value of the datatype it is
/// declared as.
pub fn stored_array_members(json: &StoredJson, value_type: Option<&NgsiLdReference>) -> Result<Vec<ArrayMember>, StoredArrayMemberError> {
    let text = json.get();
    if !text.starts_with('[') {
        return Ok(Vec::new());
    }
    serde_json::from_str::<Vec<&RawValue>>(text)?
        .into_iter()
        .filter_map(|member| array_member(member, value_type).transpose())
        .collect()
}

/// The value `member` holds: itself when it is a scalar, its `@value` when it
/// is a node whose `@value` is one, read as the value `value_type` or its own
/// `@type` declares.
fn array_member(member: &RawValue, value_type: Option<&NgsiLdReference>) -> Result<Option<ArrayMember>, StoredArrayMemberError> {
    if !member.get().starts_with('{') {
        return scalar(member)?.map(|held| declared(held, None, value_type)).transpose().map(Option::flatten);
    }
    let node = serde_json::from_str::<ValueNode<'_>>(member.get())?;
    let datatype = node.datatype.and_then(|datatype| datatype.parse::<NgsiLdReference>().ok());
    node.value
        .map(scalar)
        .transpose()?
        .flatten()
        .map(|held| declared(held, datatype.as_ref(), value_type))
        .transpose()
        .map(Option::flatten)
}

/// The member `held` is under its own `datatype` and the `value_type` beside
/// it: a string they declare as another data type is that value, and every
/// other member is itself.
fn declared(
    held: ArrayMember,
    datatype: Option<&NgsiLdReference>,
    value_type: Option<&NgsiLdReference>,
) -> Result<Option<ArrayMember>, StoredArrayMemberError> {
    let ArrayMember::Text(text) = held else {
        return Ok(Some(held));
    };
    Ok(
        match DeclaredValue::of(LiteralDeclaration::new(datatype, value_type), PrimitiveLiteral::String(&text)).transpose()? {
            None => Some(ArrayMember::Text(text)),
            Some(DeclaredValue::Time(value)) => Some(ArrayMember::Time(value)),
            Some(DeclaredValue::Number {
                number: DeclaredNumber::Finite(number),
                ..
            }) => Some(ArrayMember::Number(number)),
            Some(DeclaredValue::Number {
                number: DeclaredNumber::NonFinite(_),
                ..
            }) => None,
            Some(DeclaredValue::Boolean(flag)) => Some(ArrayMember::Boolean(flag)),
        },
    )
}

/// `value` when it is a number, a string or a boolean.
///
/// The text is JSON its codec wrote, which never opens with whitespace, so its
/// first byte names its type.
fn scalar(value: &RawValue) -> Result<Option<ArrayMember>, StoredArrayMemberError> {
    let text = value.get();
    match text.as_bytes().first() {
        Some(b'"') => Ok(Some(ArrayMember::Text(serde_json::from_str(text)?))),
        Some(b't' | b'f') => Ok(Some(ArrayMember::Boolean(serde_json::from_str(text)?))),
        Some(b'-' | b'0'..=b'9') => Ok(Some(ArrayMember::Number(text.parse()?))),
        Some(_) | None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        stored_array_members::{ArrayMember, stored_array_members},
        stored_json::StoredJson,
    };
    use andromeda_ngsi_ld_lexicon::time::value::TimeValueForm;
    use andromeda_ngsi_ld_number::JsonNumber;
    use pretty_assertions::{assert_eq, assert_ne};
    use serde_json::value::RawValue;

    fn members(text: &str) -> Vec<ArrayMember> {
        let json = StoredJson::new(RawValue::from_string(text.to_owned()).expect("JSON"));
        stored_array_members(&json, None).expect("the members read")
    }

    fn number(text: &str) -> ArrayMember {
        ArrayMember::Number(text.parse::<JsonNumber>().expect("a number"))
    }

    #[test]
    fn each_scalar_member_and_scalar_value_node_is_a_value() {
        assert_eq!(
            members(r#"[1,"a",true,{"@value":2},{"@value":"b","@language":"en"},{"@id":"urn:x"},null,[3],{"k":1},{"@value":{"k":1}}]"#),
            vec![
                number("1"),
                ArrayMember::Text("a".to_owned()),
                ArrayMember::Boolean(true),
                number("2"),
                ArrayMember::Text("b".to_owned()),
            ]
        );
    }

    /// Annex C.6: a value node's `@type` declares its datatype.
    #[test]
    fn a_value_node_typed_as_a_time_value_is_that_time_value() {
        let read = members(
            r#"[{"@value":"12:00:00.500Z","@type":"https://uri.etsi.org/ngsi-ld/Time"},{"@value":"2024-01-01","@type":"https://uri.etsi.org/ngsi-ld/Date"},"2024-01-01"]"#,
        );
        assert_eq!(
            read,
            vec![
                ArrayMember::Time(TimeValueForm::Time.parse("12:00:00.500Z").expect("a Time")),
                ArrayMember::Time(TimeValueForm::Date.parse("2024-01-01").expect("a Date")),
                ArrayMember::Text("2024-01-01".to_owned()),
            ]
        );
    }

    #[test]
    fn a_value_that_is_not_an_array_holds_no_member() {
        assert_eq!(members(r#"{"k":[1,2]}"#), Vec::new());
    }

    #[test]
    fn spaced_json_reads_the_same_members() {
        assert_eq!(
            members(r#"[ 1 , "a" , { "@value" : false } ]"#),
            vec![number("1"), ArrayMember::Text("a".to_owned()), ArrayMember::Boolean(false)]
        );
    }

    /// A number is read at its exact value, past what a double holds.
    #[test]
    fn a_member_number_is_read_exactly() {
        assert_eq!(members("[1.00]"), vec![number("1")]);
        assert_eq!(members("[0.1000000000000000000001]"), vec![number("0.1000000000000000000001")]);
        assert_ne!(members("[0.1000000000000000000001]"), vec![number("0.1")]);
    }

    #[test]
    fn an_escaped_string_reads_as_its_text() {
        assert_eq!(members(r#"["a\"bé"]"#), vec![ArrayMember::Text("a\"b\u{e9}".to_owned())]);
    }
}
