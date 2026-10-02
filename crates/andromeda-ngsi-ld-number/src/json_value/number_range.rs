//! Refusing a JSON text holding a number outside the supported range, before
//! anything is built from it.
//!
//! [`canonicalize_numbers`](crate::json_value::canonicalize_numbers) checks the
//! numbers of a parsed tree; a request decoded straight from its text into a
//! domain type has no tree to walk, and many of its numbers -- the ordinates of
//! a geometry, a typed member -- never become a JSON number again. This walk
//! drives `serde_json`'s streaming reader through every value of the text and
//! refuses the first number [`JsonNumber`](crate::JsonNumber) cannot hold, at
//! the same RFC 6901 location and with the same reason the tree walk gives.
//!
//! A member named with the prefix `serde_json`'s reader reserves for its own
//! number and raw-value tokens is refused as well, wherever it is not the
//! reader's own spelling of a number: a tree holding such a member reads back
//! as a different value once its members are reordered, as a stored document's
//! are.
//!
//! The walk reads the whole text even after a refusal, so a text that is not
//! JSON at all is refused for its syntax, which a parse of it refuses first.

use crate::{
    JsonNumberError,
    checked_decimal_from_json,
    json_value::{
        JsonNumberPathError,
        decode_route::SERDE_JSON_PRIVATE_MEMBER_PREFIX,
        json_pointer::{JsonPointer, JsonPointerSegment},
        value_census::MemberName,
    },
    number_token::NumberToken,
};
use serde::de::{DeserializeSeed, Deserializer, Error as DeserializeError, MapAccess, SeqAccess, Visitor};
use serde_json::Deserializer as JsonReader;
use std::{cell::RefCell, fmt};
use thiserror::Error;

/// The reader's refusal of a number token that follows no JSON number
/// grammar.
const UNREADABLE_TOKEN: &str = "invalid number";

/// Why a JSON text's numbers are refused.
#[derive(Debug, Error)]
pub enum NumberRangeError {
    /// The text holds a number outside the supported range, at this location.
    #[error(transparent)]
    OutOfRange(#[from] JsonNumberPathError),
    /// The text names a member with the prefix the reader reserves for its own
    /// number and raw-value tokens, at this location.
    #[error("the member at {path} is named with the prefix the JSON reader reserves for its own tokens")]
    ReservedMemberName {
        /// JSON Pointer locating the member.
        path: JsonPointer,
    },
    /// The text is not one well-formed JSON value as `serde_json`'s reader
    /// reads it.
    #[error(transparent)]
    Syntax(#[from] serde_json::Error),
}

/// The first refusal one walk met.
#[derive(Debug)]
enum Refusal {
    /// A number outside the supported range.
    Number(JsonNumberPathError),
    /// A member named with the reader's reserved prefix, at this location.
    ReservedMemberName(JsonPointer),
}

/// Refuse a JSON text holding a number outside the supported range.
///
/// # Errors
///
/// Returns [`NumberRangeError::Syntax`] for a text the streaming reader
/// refuses, trailing content included, and otherwise the first refusal in
/// document order: [`NumberRangeError::OutOfRange`] locating a number that lies
/// outside the supported range, or [`NumberRangeError::ReservedMemberName`]
/// locating a member named with the reader's reserved prefix.
pub fn refuse_numbers_out_of_range(text: &[u8]) -> Result<(), NumberRangeError> {
    let walk = RangeWalk::default();
    let mut reader = JsonReader::from_slice(text);
    RangeSeed { walk: &walk }.deserialize(&mut reader).and_then(|()| reader.end())?;
    match walk.refused.into_inner() {
        None => Ok(()),
        Some(Refusal::Number(refused)) => Err(NumberRangeError::OutOfRange(refused)),
        Some(Refusal::ReservedMemberName(path)) => Err(NumberRangeError::ReservedMemberName { path }),
    }
}

/// The state of one walk: where it stands, and the first refusal it met.
#[derive(Debug, Default)]
struct RangeWalk {
    /// The location of the value being read, root first.
    path: RefCell<Vec<JsonPointerSegment>>,
    /// The first refusal met.
    refused: RefCell<Option<Refusal>>,
}

impl RangeWalk {
    /// Check one number token, recording the first refusal.
    ///
    /// A token without an exponent whose digits lie in range is accepted
    /// without building its decimal, so a text of millions of ordinates is
    /// checked without an allocation per number.
    ///
    /// # Errors
    ///
    /// Returns the reader's error for a token that is no JSON number at all,
    /// which only an object spelling out the reader's private number member
    /// can carry and which a parse of the text refuses as well.
    fn check<E: DeserializeError>(&self, token: &str) -> Result<(), E> {
        let Some(parsed) = NumberToken::parse(token) else {
            return Err(E::custom(UNREADABLE_TOKEN));
        };
        if self.refused.borrow().is_some() || parsed.is_plainly_within_range() {
            return Ok(());
        }
        if let Err(source) = checked_decimal_from_json(token) {
            self.refuse(source);
        }
        Ok(())
    }

    /// Record `source` as the refusal of the number at the current location.
    fn refuse(&self, source: JsonNumberError) {
        let path = self.location();
        *self.refused.borrow_mut() = Some(Refusal::Number(JsonNumberPathError { path, source }));
    }

    /// Refuse the member `name` of the object at the current location if the
    /// reader reserves its prefix, unless an earlier refusal stands.
    fn check_name(&self, name: &str) {
        if self.refused.borrow().is_none() && name.starts_with(SERDE_JSON_PRIVATE_MEMBER_PREFIX) {
            // clone: the refusal owns the location the walk goes on to change
            let mut path = self.path.borrow().clone();
            path.push(JsonPointerSegment::Member(Box::from(name)));
            *self.refused.borrow_mut() = Some(Refusal::ReservedMemberName(JsonPointer::from_segments(path)));
        }
    }

    /// The location of the value being read.
    fn location(&self) -> JsonPointer {
        // clone: the refusal owns the location the walk goes on to change
        JsonPointer::from_segments(self.path.borrow().clone())
    }

    /// Read the value at `segment` below the current location with `read`.
    fn below<T, E>(&self, segment: JsonPointerSegment, read: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
        self.path.borrow_mut().push(segment);
        let read = read();
        self.path.borrow_mut().pop();
        read
    }
}

/// Reads one value and everything below it, checking every number.
#[derive(Clone, Copy, Debug)]
struct RangeSeed<'w> {
    /// The walk this value belongs to.
    walk: &'w RangeWalk,
}

impl<'de> DeserializeSeed<'de> for RangeSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for RangeSeed<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E: DeserializeError>(self, _value: bool) -> Result<(), E> {
        Ok(())
    }

    // A 64-bit integer reaches the visitor as itself rather than as a token,
    // and every one of them lies within the supported range.
    fn visit_i64<E: DeserializeError>(self, _value: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E: DeserializeError>(self, _value: u64) -> Result<(), E> {
        Ok(())
    }

    fn visit_str<E: DeserializeError>(self, _value: &str) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E: DeserializeError>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<(), A::Error> {
        let mut index = 0;
        while self.walk.below(JsonPointerSegment::Index(index), || items.next_element_seed(self))?.is_some() {
            index += 1;
        }
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<(), A::Error> {
        let Some(first) = members.next_key_seed(MemberNameText)? else {
            return Ok(());
        };
        // The reader's spelling of one number, not an object.
        if MemberName::of(&first) == MemberName::NumberToken {
            let token = members.next_value_seed(MemberNameText)?;
            return self.walk.check(&token);
        }
        let mut name = first;
        loop {
            self.walk.check_name(&name);
            self.walk.below(JsonPointerSegment::Member(name), || members.next_value_seed(self))?;
            match members.next_key_seed(MemberNameText)? {
                Some(next) => name = next,
                None => return Ok(()),
            }
        }
    }
}

/// Reads one member name, or the text of a number token, as owned text.
#[derive(Clone, Copy, Debug)]
struct MemberNameText;

impl<'de> DeserializeSeed<'de> for MemberNameText {
    type Value = Box<str>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Box<str>, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for MemberNameText {
    type Value = Box<str>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a member name")
    }

    fn visit_str<E: DeserializeError>(self, name: &str) -> Result<Box<str>, E> {
        Ok(Box::from(name))
    }
}

#[cfg(test)]
mod tests {
    use crate::json_value::{
        canonicalize_numbers,
        number_range::{NumberRangeError, refuse_numbers_out_of_range},
    };
    use serde_json::Value;

    /// The refusal a tree walk gives for `text`, rendered.
    fn tree_refusal(text: &str) -> Option<String> {
        let mut value: Value = serde_json::from_str(text).expect("valid JSON");
        canonicalize_numbers(&mut value).err().map(|refused| refused.to_string())
    }

    /// The refusal the text walk gives for `text`, rendered.
    fn text_refusal(text: &str) -> Option<String> {
        match refuse_numbers_out_of_range(text.as_bytes()) {
            Ok(()) => None,
            Err(NumberRangeError::OutOfRange(refused)) => Some(refused.to_string()),
            Err(refused @ (NumberRangeError::Syntax(_) | NumberRangeError::ReservedMemberName { .. })) => panic!("{text}: unexpected refusal {refused}"),
        }
    }

    #[test]
    fn a_text_of_supported_numbers_passes() {
        for text in [
            r#"{"a":[1,-2,0.5,1e-7,-2.5E-400,18446744073709551616,1.234567890123456789e20,-0]}"#,
            "42",
            "[]",
            r#"{"id":"urn:ngsi-ld:T:1","n":{"type":"Property","value":[21.5,"x",null,true,{}]}}"#,
        ] {
            assert!(refuse_numbers_out_of_range(text.as_bytes()).is_ok(), "{text}");
        }
    }

    /// The text walk refuses what the tree walk refuses, at the same
    /// location and for the same reason.
    #[test]
    fn an_out_of_range_number_is_refused_where_the_tree_walk_refuses_it() {
        for text in [
            r#"{"property":{"value":[0,{"too/large":1e-16384}]}}"#,
            "1e-16384",
            r#"{"a~b":{"c/d~":[1,2,1e-16384]}}"#,
            r#"{"location":{"type":"GeoProperty","value":{"type":"Point","coordinates":[1e200000,2]}}}"#,
            r#"[{"id":"x"},{"n":[[1e140000]]}]"#,
        ] {
            let refused = text_refusal(text);
            assert!(refused.is_some(), "{text}");
            pretty_assertions::assert_eq!(refused, tree_refusal(text), "{text}");
        }
    }

    /// A text that is not JSON is refused for its syntax even when a number
    /// before the fault is out of range, as a parse of it would refuse it.
    #[test]
    fn a_syntax_error_after_an_out_of_range_number_is_a_syntax_error() {
        assert!(matches!(refuse_numbers_out_of_range(b"[1e-16384,"), Err(NumberRangeError::Syntax(_))));
        assert!(matches!(refuse_numbers_out_of_range(b"[1] x"), Err(NumberRangeError::Syntax(_))));
    }

    #[test]
    fn an_object_whose_first_member_is_named_like_a_number_token_is_read_as_one() {
        assert!(refuse_numbers_out_of_range(br#"{"v":{"$serde_json::private::Number":"5"}}"#).is_ok());
    }

    /// A member named with the reader's reserved prefix anywhere but in its
    /// own spelling of a number is refused at its location.
    #[test]
    fn a_member_named_with_the_readers_reserved_prefix_is_refused_where_it_stands() {
        for (text, location) in [
            (r#"{"v":{"b":1,"$serde_json::private::Number":"5"}}"#, "/v/$serde_json::private::Number"),
            (r#"[{"$serde_json::private::RawValue":"[1]"}]"#, "/0/$serde_json::private::RawValue"),
        ] {
            match refuse_numbers_out_of_range(text.as_bytes()) {
                Err(NumberRangeError::ReservedMemberName { path }) => pretty_assertions::assert_eq!(path.to_string(), location, "{text}"),
                other => panic!("{text}: expected a reserved member name, got {other:?}"),
            }
        }
    }
}
