//! The clause 4.9 target value a Query Term compares its operand with, and the
//! readings clause 4.9 gives a target holding several values.
//!
//! What each attribute family's target is, is
//! [`family_target`](crate::filter::family_target)'s.

use crate::queried_entity::value_item::LiteralValue;
use andromeda_json_literal::piece::{JsonKind, JsonPiece};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time::value::TimeValue};
use andromeda_ngsi_ld_number::{JsonNumber, declared_number::NonFinite};
use std::{borrow::Cow, cmp::Ordering};

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
    Json(JsonPiece<'a>),
    /// Several values, which clause 4.9 compares by membership.
    Array(Vec<TargetValue<'a>>),
    /// A value declared as a clause 4.6.3 time value, or an instant the
    /// instance holds; only a time operand of its form compares with it.
    Time(TimeValue),
    /// A number beyond the decimals, declared as `xsd:float` or `xsd:double`.
    NonFiniteNumber(NonFinite),
}

impl<'a> TargetValue<'a> {
    /// The target value raw JSON holds, a JSON array becoming the array of its
    /// elements.
    #[must_use]
    pub fn json(piece: JsonPiece<'a>) -> TargetValue<'a> {
        match piece.elements() {
            Some(elements) => TargetValue::Array(elements.into_iter().map(TargetValue::json).collect()),
            None => TargetValue::Json(piece),
        }
    }

    /// The target value a literal holds.
    pub(crate) fn literal(literal: LiteralValue<'a>) -> TargetValue<'a> {
        match literal {
            LiteralValue::Number(number) => TargetValue::Number(number),
            LiteralValue::Text(text) => TargetValue::Text(text),
            LiteralValue::Boolean(boolean) => TargetValue::Boolean(boolean),
            LiteralValue::Time(value) => TargetValue::Time(value),
            LiteralValue::NonFiniteNumber(value) => TargetValue::NonFiniteNumber(value),
        }
    }

    /// The value when it is a finite number.
    ///
    /// A string spelling a number is a String, not a number: clause 4.9 makes a
    /// target of another data type unequal to the operand.
    #[must_use]
    pub fn as_number(&self) -> Option<Cow<'_, JsonNumber>> {
        match self {
            TargetValue::Number(number) => Some(Cow::Borrowed(number.as_ref())),
            TargetValue::Json(json) => match json.kind() {
                JsonKind::Number => json.as_number().map(Cow::Owned),
                JsonKind::Null | JsonKind::String | JsonKind::Boolean | JsonKind::Array | JsonKind::Object => None,
            },
            TargetValue::Text(_) | TargetValue::Boolean(_) | TargetValue::Array(_) | TargetValue::Time(_) | TargetValue::NonFiniteNumber(_) => None,
        }
    }

    /// How the value orders against the finite number `operand`, `None` when it
    /// is no number or is not-a-number.
    ///
    /// XSD orders positive infinity above every finite number and negative
    /// infinity below; not-a-number is unordered, so it fails `==` and every
    /// ordering and satisfies `!=`.
    #[must_use]
    pub fn number_ordering(&self, operand: &JsonNumber) -> Option<Ordering> {
        match self {
            TargetValue::NonFiniteNumber(NonFinite::PositiveInfinity) => Some(Ordering::Greater),
            TargetValue::NonFiniteNumber(NonFinite::NegativeInfinity) => Some(Ordering::Less),
            TargetValue::NonFiniteNumber(NonFinite::NotANumber) => None,
            TargetValue::Number(_) | TargetValue::Text(_) | TargetValue::Boolean(_) | TargetValue::Json(_) | TargetValue::Array(_) | TargetValue::Time(_) => {
                Some(self.as_number()?.as_ref().cmp(operand))
            }
        }
    }

    /// The value when it is a boolean; a string spelling one is a String.
    #[must_use]
    pub fn as_boolean(&self) -> Option<bool> {
        match self {
            TargetValue::Boolean(boolean) => Some(*boolean),
            TargetValue::Json(json) => match json.kind() {
                JsonKind::Boolean => json.as_bool(),
                JsonKind::Null | JsonKind::String | JsonKind::Number | JsonKind::Array | JsonKind::Object => None,
            },
            TargetValue::Text(_) | TargetValue::Number(_) | TargetValue::Array(_) | TargetValue::Time(_) | TargetValue::NonFiniteNumber(_) => None,
        }
    }

    /// The value when it is a time value.
    #[must_use]
    pub const fn as_time(&self) -> Option<TimeValue> {
        match self {
            TargetValue::Time(value) => Some(*value),
            TargetValue::Number(_)
            | TargetValue::Text(_)
            | TargetValue::Boolean(_)
            | TargetValue::Json(_)
            | TargetValue::Array(_)
            | TargetValue::NonFiniteNumber(_) => None,
        }
    }

    /// The value when it is a String, the only type a String or URI operand
    /// and a pattern operator compare with (clause 4.9); a number or a boolean
    /// is no String, whatever its spelling.
    #[must_use]
    pub fn as_text(&self) -> Option<Cow<'_, str>> {
        match self {
            TargetValue::Text(text) => Some(Cow::Borrowed(text.as_ref())),
            TargetValue::Json(json) => json.as_str(),
            TargetValue::Number(_) | TargetValue::Boolean(_) | TargetValue::Array(_) | TargetValue::Time(_) | TargetValue::NonFiniteNumber(_) => None,
        }
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
pub fn json_member<'a>(seed: &ResolvedTarget<'a>, keys: &[AttributeName]) -> Option<ResolvedTarget<'a>> {
    let TargetValue::Json(json) = seed.value else {
        return None;
    };
    Some(ResolvedTarget {
        value: TargetValue::json(json.member_path(keys.iter().map(AttributeName::as_str))?),
        origin: seed.origin,
        reading: seed.reading,
    })
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::filter::target_value::{MultiValueReading, ResolvedTarget, TargetOrigin, TargetValue, json_member};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use std::borrow::Cow;

    fn keys(names: &[&str]) -> Vec<AttributeName> {
        names.iter().map(|name| name.parse().expect("a key")).collect()
    }

    fn literal(text: &str) -> JsonLiteral {
        JsonLiteral::admit(text).expect("a literal")
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
        let document = literal(r#"[1, [2, "x"]]"#);
        let (one, two, x) = (literal("1"), literal("2"), literal(r#""x""#));
        pretty_assertions::assert_eq!(
            TargetValue::json(document.piece()),
            TargetValue::Array(vec![
                TargetValue::Json(one.piece()),
                TargetValue::Array(vec![TargetValue::Json(two.piece()), TargetValue::Json(x.piece())]),
            ])
        );
    }

    /// Clause 4.9 makes a target of another data type unequal to the operand,
    /// so a value reads only in the domain of its own JSON type.
    #[test]
    fn a_target_reads_only_in_the_domain_of_its_own_json_type() {
        let (twelve, object, text, yes) = (literal("12"), literal(r#"{"a": 1}"#), literal(r#""12""#), literal("true"));
        pretty_assertions::assert_eq!(TargetValue::Json(twelve.piece()).as_number().map(Cow::into_owned), "12".parse().ok());
        pretty_assertions::assert_eq!(TargetValue::Json(twelve.piece()).as_text(), None);
        pretty_assertions::assert_eq!(TargetValue::Json(text.piece()).as_number(), None);
        pretty_assertions::assert_eq!(TargetValue::Json(text.piece()).as_text().as_deref(), Some("12"));
        pretty_assertions::assert_eq!(TargetValue::Json(yes.piece()).as_boolean(), Some(true));
        pretty_assertions::assert_eq!(TargetValue::Json(yes.piece()).as_text(), None);
        pretty_assertions::assert_eq!(TargetValue::Json(object.piece()).as_text(), None);
        pretty_assertions::assert_eq!(TargetValue::Text(Cow::Borrowed("0.5")).as_number(), None);
        pretty_assertions::assert_eq!(TargetValue::Text(Cow::Borrowed("true")).as_boolean(), None);
        pretty_assertions::assert_eq!(TargetValue::Number(Cow::Owned("12".parse().expect("a number"))).as_text(), None);
        pretty_assertions::assert_eq!(TargetValue::Boolean(false).as_text(), None);
    }

    /// Raw JSON members are addressed by their verbatim keys; anything that is
    /// not a JSON object has no members.
    #[test]
    fn a_json_member_is_found_by_its_verbatim_key_in_an_object_only() {
        let (document, three) = (literal(r#"{"a": {"b": 3}}"#), literal("3"));
        let found = json_member(&seed(TargetValue::json(document.piece())), &keys(&["a", "b"])).expect("a member");
        pretty_assertions::assert_eq!(found.value, TargetValue::Json(three.piece()));
        let (flat, listed) = (literal(r#"{"a": 1}"#), literal(r#"[{"a": 1}]"#));
        assert!(json_member(&seed(TargetValue::json(flat.piece())), &keys(&["a", "b"])).is_none());
        assert!(json_member(&seed(TargetValue::Text(Cow::Borrowed("a"))), &keys(&["a"])).is_none());
        assert!(json_member(&seed(TargetValue::json(listed.piece())), &keys(&["a"])).is_none());
    }
}
