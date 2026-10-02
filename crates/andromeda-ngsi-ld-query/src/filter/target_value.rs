//! The clause 4.9 target value a Query Term compares its operand with, and the
//! readings clause 4.9 gives a target holding several values.
//!
//! What each attribute family's target is, is
//! [`family_target`](crate::filter::family_target)'s.

use crate::queried_entity::value_item::LiteralValue;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_number::JsonNumber;
use serde_json::Value;
use std::borrow::Cow;

/// One resolved comparison target: the clause 4.9 target value a predicate
/// compares, and where it came from.
///
/// Clause 4.9 forbids the ordering and pattern operators against a
/// Relationship, so the evaluator has to know a value's origin, not only its
/// shape — a Relationship object and a string Property value are both strings.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTarget<'a> {
    /// The target value to compare against the Query Term operand.
    pub value: TargetValue<'a>,
    /// Which clause 4.9 definition produced it.
    pub origin: TargetOrigin,
    /// What clause 4.9 makes of it when it holds several values.
    pub reading: MultiValueReading,
}

/// A clause 4.9 target value, in the JSON types a Query Term operand is
/// compared with.
#[derive(Debug, Clone, PartialEq)]
pub enum TargetValue<'a> {
    /// An exact number (clause 4.6.3).
    Number(Cow<'a, JsonNumber>),
    /// A string, or the lexical form of an IRI or of a typed literal.
    Text(Cow<'a, str>),
    /// A boolean.
    Boolean(bool),
    /// Raw JSON held outside the JSON-LD graph, never an array: a JSON array is
    /// an [`TargetValue::Array`] of its elements, which [`TargetValue::json`]
    /// guarantees.
    Json(Cow<'a, Value>),
    /// Several values, which clause 4.9 compares by membership.
    Array(Vec<TargetValue<'a>>),
}

impl<'a> TargetValue<'a> {
    /// The target value raw JSON holds, a JSON array becoming the array of its
    /// elements.
    #[must_use]
    pub fn json(value: Cow<'a, Value>) -> TargetValue<'a> {
        match value {
            Cow::Borrowed(Value::Array(elements)) => TargetValue::Array(elements.iter().map(|element| TargetValue::json(Cow::Borrowed(element))).collect()),
            Cow::Owned(Value::Array(elements)) => TargetValue::Array(elements.into_iter().map(|element| TargetValue::json(Cow::Owned(element))).collect()),
            Cow::Borrowed(scalar @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_))) => {
                TargetValue::Json(Cow::Borrowed(scalar))
            }
            Cow::Owned(scalar @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_))) => {
                TargetValue::Json(Cow::Owned(scalar))
            }
        }
    }

    /// The target value a literal holds.
    pub(crate) fn literal(literal: LiteralValue<'a>) -> TargetValue<'a> {
        match literal {
            LiteralValue::Number(number) => TargetValue::Number(number),
            LiteralValue::Text(text) => TargetValue::Text(text),
            LiteralValue::Boolean(boolean) => TargetValue::Boolean(boolean),
        }
    }

    /// The value as an exact number, accepting the textual form typed JSON-LD
    /// values use.
    #[must_use]
    pub fn as_number(&self) -> Option<Cow<'_, JsonNumber>> {
        match self {
            TargetValue::Number(number) => Some(Cow::Borrowed(number.as_ref())),
            TargetValue::Text(text) => text.parse().ok().map(Cow::Owned),
            TargetValue::Json(json) => match json.as_ref() {
                Value::Number(number) => JsonNumber::try_from(number).ok().map(Cow::Owned),
                Value::String(text) => text.parse().ok().map(Cow::Owned),
                Value::Null | Value::Bool(_) | Value::Array(_) | Value::Object(_) => None,
            },
            TargetValue::Boolean(_) | TargetValue::Array(_) => None,
        }
    }

    /// The value as a boolean, accepting its textual form.
    #[must_use]
    pub fn as_boolean(&self) -> Option<bool> {
        match self {
            TargetValue::Boolean(boolean) => Some(*boolean),
            TargetValue::Text(text) => boolean_spelled(text),
            TargetValue::Json(json) => match json.as_ref() {
                Value::Bool(boolean) => Some(*boolean),
                Value::String(text) => boolean_spelled(text),
                Value::Null | Value::Number(_) | Value::Array(_) | Value::Object(_) => None,
            },
            TargetValue::Number(_) | TargetValue::Array(_) => None,
        }
    }

    /// The value rendered as text for string and temporal comparisons: a
    /// string as it is, a number or a boolean in its JSON spelling.
    #[must_use]
    pub fn as_lexical(&self) -> Option<Cow<'_, str>> {
        match self {
            TargetValue::Text(text) => Some(Cow::Borrowed(text.as_ref())),
            TargetValue::Number(number) => Some(Cow::Owned(number.to_string())),
            TargetValue::Boolean(boolean) => Some(Cow::Owned(boolean.to_string())),
            TargetValue::Json(json) => match json.as_ref() {
                Value::String(text) => Some(Cow::Borrowed(text.as_str())),
                Value::Number(number) => Some(Cow::Owned(number.to_string())),
                Value::Bool(boolean) => Some(Cow::Owned(boolean.to_string())),
                Value::Null | Value::Array(_) | Value::Object(_) => None,
            },
            TargetValue::Array(_) => None,
        }
    }

    /// The value when it is a String, the only type a pattern operator applies
    /// to (clause 4.9).
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            TargetValue::Text(text) => Some(text.as_ref()),
            TargetValue::Json(json) => json.as_str(),
            TargetValue::Number(_) | TargetValue::Boolean(_) | TargetValue::Array(_) => None,
        }
    }
}

/// A boolean in its textual spelling.
fn boolean_spelled(text: &str) -> Option<bool> {
    match text {
        "true" => Some(true),
        "false" => Some(false),
        // `&str` is unbounded, so a catch-all is the only way to close this
        // match; anything else is not a boolean in any spelling.
        _ => None,
    }
}

/// What clause 4.9 makes of an attribute that carries several values.
///
/// The two readings agree on the equality operators and part company on every
/// other one, so which of them applies is a property of the target rather than
/// of the operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiValueReading {
    /// The array of values is itself the target value.
    ///
    /// Clause 4.9 defines a `ListProperty`'s target value as "the valueList
    /// array associated to such a `ListProperty`" and a `ListRelationship`'s as
    /// "the array of objects", and a Property's is the Value the client wrote —
    /// an array when they wrote one. The equality operators test membership in
    /// it ("Is included in the target value, if the latter is an array"), while
    /// an operator defined over a scalar or a String target does not match it at
    /// all: "If the target value data type is different than String then it
    /// shall be considered as not matching."
    WholeArray,
    /// Any one of the values is the target value in its own right.
    ///
    /// Clause 4.9: "If the target element is a `LanguageProperty`, and no target
    /// language is specified, the target value is defined as a value from any of
    /// the key-value pairs held within the languageMap." Every operator is
    /// therefore answered by the values themselves, and the negated ones hold
    /// only when none of them satisfies the Query Term — "No matching value is
    /// found in any of the values of the key-value pairs of a languageMap".
    AnyOneValue,
}

/// Which clause 4.9 definition a resolved target came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOrigin {
    /// A target *value*: what a Property-family attribute carries.
    AttributeValue,
    /// A target *object*: the URI, or array of URIs, a Relationship or a
    /// `ListRelationship` names.
    RelationshipObject,
}

/// Address a member of raw JSON by the keys the client wrote, starting from
/// the target value a path resolved to.
///
/// Only a JSON object has members; any other target addresses nothing. The
/// member keeps the origin and the reading of the target it was found in.
#[must_use]
pub fn json_member<'a>(seed: ResolvedTarget<'a>, keys: &[AttributeName]) -> Option<ResolvedTarget<'a>> {
    let TargetValue::Json(mut json) = seed.value else {
        return None;
    };
    for key in keys {
        json = match json {
            Cow::Borrowed(value) => Cow::Borrowed(value.get(key.as_str())?),
            Cow::Owned(mut value) => Cow::Owned(value.get_mut(key.as_str())?.take()),
        };
    }
    Some(ResolvedTarget {
        value: TargetValue::json(json),
        origin: seed.origin,
        reading: seed.reading,
    })
}

#[cfg(test)]
mod tests {
    use crate::filter::target_value::{MultiValueReading, ResolvedTarget, TargetOrigin, TargetValue, json_member};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use serde_json::json;
    use std::borrow::Cow;

    fn keys(names: &[&str]) -> Vec<AttributeName> {
        names.iter().map(|name| name.parse().expect("a key")).collect()
    }

    fn seed(value: TargetValue<'_>) -> ResolvedTarget<'_> {
        ResolvedTarget {
            value,
            origin: TargetOrigin::AttributeValue,
            reading: MultiValueReading::WholeArray,
        }
    }

    /// A JSON array is the array of its elements, nested arrays included, so
    /// membership reaches every element.
    #[test]
    fn a_json_array_is_an_array_of_its_elements() {
        let document = json!([1, [2, "x"]]);
        pretty_assertions::assert_eq!(
            TargetValue::json(Cow::Borrowed(&document)),
            TargetValue::Array(vec![
                TargetValue::Json(Cow::Owned(json!(1))),
                TargetValue::Array(vec![TargetValue::Json(Cow::Owned(json!(2))), TargetValue::Json(Cow::Owned(json!("x")))]),
            ])
        );
    }

    /// Numbers, booleans and strings read in each other's domains exactly as
    /// their JSON spellings allow.
    #[test]
    fn a_target_reads_in_the_domain_of_the_operand() {
        let number = TargetValue::Text(Cow::Borrowed("0.5"));
        pretty_assertions::assert_eq!(number.as_number().map(Cow::into_owned), "0.5".parse().ok());
        pretty_assertions::assert_eq!(TargetValue::Text(Cow::Borrowed("true")).as_boolean(), Some(true));
        pretty_assertions::assert_eq!(TargetValue::Boolean(false).as_lexical().as_deref(), Some("false"));
        pretty_assertions::assert_eq!(TargetValue::Json(Cow::Owned(json!(12))).as_lexical().as_deref(), Some("12"));
        pretty_assertions::assert_eq!(TargetValue::Json(Cow::Owned(json!({"a": 1}))).as_lexical(), None);
        pretty_assertions::assert_eq!(TargetValue::Json(Cow::Owned(json!(12))).as_text(), None);
    }

    /// Raw JSON members are addressed by their verbatim keys; anything that is
    /// not a JSON object has no members.
    #[test]
    fn a_json_member_is_found_by_its_verbatim_key_in_an_object_only() {
        let document = json!({"a": {"b": 3}});
        let found = json_member(seed(TargetValue::json(Cow::Borrowed(&document))), &keys(&["a", "b"])).expect("a member");
        pretty_assertions::assert_eq!(found.value, TargetValue::Json(Cow::Owned(json!(3))));
        assert!(json_member(seed(TargetValue::json(Cow::Owned(json!({"a": 1})))), &keys(&["a", "b"])).is_none());
        assert!(json_member(seed(TargetValue::Text(Cow::Borrowed("a"))), &keys(&["a"])).is_none());
        assert!(json_member(seed(TargetValue::json(Cow::Owned(json!([{"a": 1}])))), &keys(&["a"])).is_none());
    }
}
