//! Admitting a request's JSON text before anything is built from it: no number
//! outside the supported range, no reserved reader-token member name, no
//! repeated member name.
//!
//! A request decoded straight from its text into a domain type has no tree to
//! walk, and many of its numbers -- the ordinates of a geometry, a typed
//! member -- never become a JSON number again. This walk drives `serde_json`'s
//! streaming reader through every value of the text and refuses the first
//! number [`JsonNumber`](crate::JsonNumber) cannot hold, at its RFC 6901
//! location.
//!
//! A member named with the prefix `serde_json`'s reader reserves for its own
//! number and raw-value tokens is refused as well, wherever it is not the
//! reader's own spelling of a number: a tree holding such a member reads back
//! as a different value once its members are reordered, as a stored document's
//! are.
//!
//! An object naming one member twice is refused as well, at the location of
//! the repeated member: IETF RFC 8259 section 4 asks member names within an
//! object to be unique and leaves the meaning of a repeated one undefined,
//! while the readers of a request text each keep a different one of the two.
//! The names are already read by this walk, so a request text is checked for
//! repeats in the pass that checks its numbers, and no reader of it can
//! disagree with another about which value a repeated name carries.
//!
//! The walk reads the whole text even after a refusal, so a text that is not
//! JSON at all is refused for its syntax, which a parse of it refuses first.

use crate::{
    JsonNumberError,
    checked_decimal_from_json,
    json_value::{
        json_pointer::{JsonPointer, JsonPointerSegment},
        reader_private_members::SERDE_JSON_PRIVATE_MEMBER_PREFIX,
        value_census::MemberName,
    },
    number_token::NumberToken,
};
use serde::de::{DeserializeSeed, Deserializer, Error as DeserializeError, MapAccess, SeqAccess, Visitor};
use serde_json::Deserializer as JsonReader;
use std::{cell::RefCell, collections::HashSet, fmt};
use thiserror::Error;

/// A rejected JSON number and its JSON Pointer location.
#[derive(Debug, Error)]
#[error("the JSON number at {path} is refused: {source}")]
pub struct JsonNumberPathError {
    /// JSON Pointer locating the rejected member.
    pub path: JsonPointer,
    /// The exact-number syntax or supported-range failure.
    #[source]
    pub source: JsonNumberError,
}

/// The reader's refusal of a number token that follows no JSON number
/// grammar.
const UNREADABLE_TOKEN: &str = "invalid number";

/// Why a request's JSON text is refused.
#[derive(Debug, Error)]
pub enum JsonTextRefusal {
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
    /// An object names one member twice; the location is the repeated member,
    /// the second of the two.
    #[error("the member at {path} is named twice in its object, and IETF RFC 8259 section 4 asks the member names of an object to be unique")]
    DuplicateMember {
        /// JSON Pointer locating the repeated member.
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
    /// A member named twice in its object, at this location.
    DuplicateMember(JsonPointer),
}

/// Admit a JSON text: refuse one holding a number outside the supported range,
/// a member named with the reader's reserved prefix, or an object that names
/// one member twice.
///
/// # Errors
///
/// Returns [`JsonTextRefusal::Syntax`] for a text the streaming reader
/// refuses, trailing content included, and otherwise the first refusal in
/// document order: [`JsonTextRefusal::OutOfRange`] locating a number that lies
/// outside the supported range, [`JsonTextRefusal::ReservedMemberName`]
/// locating a member named with the reader's reserved prefix, or
/// [`JsonTextRefusal::DuplicateMember`] locating a repeated member.
pub fn admit_json_text(text: &[u8]) -> Result<(), JsonTextRefusal> {
    let walk = AdmissionWalk::default();
    let mut reader = JsonReader::from_slice(text);
    AdmissionSeed { walk: &walk }.deserialize(&mut reader).and_then(|()| reader.end())?;
    match walk.refused.into_inner() {
        None => Ok(()),
        Some(Refusal::Number(refused)) => Err(JsonTextRefusal::OutOfRange(refused)),
        Some(Refusal::ReservedMemberName(path)) => Err(JsonTextRefusal::ReservedMemberName { path }),
        Some(Refusal::DuplicateMember(path)) => Err(JsonTextRefusal::DuplicateMember { path }),
    }
}

/// The state of one walk: where it stands, and the first refusal it met.
#[derive(Debug, Default)]
struct AdmissionWalk {
    /// The location of the value being read, root first.
    path: RefCell<Vec<JsonPointerSegment>>,
    /// The first refusal met.
    refused: RefCell<Option<Refusal>>,
}

impl AdmissionWalk {
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
            *self.refused.borrow_mut() = Some(Refusal::ReservedMemberName(self.member_location(name)));
        }
    }

    /// Refuse the member `name` of the object at the current location if
    /// `seen`, the names of that object read before it, already holds it,
    /// unless an earlier refusal stands.
    fn check_repeat(&self, seen: &mut HashSet<Box<str>>, name: &str) {
        // clone: the set owns the names of the object while the walk goes on to read the member's value
        let repeated = !seen.insert(Box::from(name));
        if repeated && self.refused.borrow().is_none() {
            *self.refused.borrow_mut() = Some(Refusal::DuplicateMember(self.member_location(name)));
        }
    }

    /// The location of the member `name` of the object at the current
    /// location.
    fn member_location(&self, name: &str) -> JsonPointer {
        // clone: the refusal owns the location the walk goes on to change
        let mut path = self.path.borrow().clone();
        path.push(JsonPointerSegment::Member(Box::from(name)));
        JsonPointer::from_segments(path)
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

/// Reads one value and everything below it, checking every number and member
/// name.
#[derive(Clone, Copy, Debug)]
struct AdmissionSeed<'w> {
    /// The walk this value belongs to.
    walk: &'w AdmissionWalk,
}

impl<'de> DeserializeSeed<'de> for AdmissionSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for AdmissionSeed<'_> {
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
        let mut seen = HashSet::new();
        loop {
            self.walk.check_name(&name);
            self.walk.check_repeat(&mut seen, &name);
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
    use crate::json_value::text_admission::{JsonTextRefusal, admit_json_text};

    /// The refusal the text walk gives for `text`, rendered.
    fn text_refusal(text: &str) -> Option<String> {
        match admit_json_text(text.as_bytes()) {
            Ok(()) => None,
            Err(JsonTextRefusal::OutOfRange(refused)) => Some(refused.to_string()),
            Err(refused @ (JsonTextRefusal::Syntax(_) | JsonTextRefusal::ReservedMemberName { .. } | JsonTextRefusal::DuplicateMember { .. })) => {
                panic!("{text}: unexpected refusal {refused}")
            }
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
            assert!(admit_json_text(text.as_bytes()).is_ok(), "{text}");
        }
    }

    /// A number outside the supported range is refused at its RFC 6901
    /// location, with the member names escaped.
    #[test]
    fn an_out_of_range_number_is_refused_at_its_json_pointer_naming_the_broken_limit() {
        for (text, location, limit) in [
            (
                r#"{"property":{"value":[0,{"too/large":1e-16384}]}}"#,
                "/property/value/1/too~1large",
                "16383 digits after",
            ),
            ("1e-16384", "/", "16383 digits after"),
            (r#"{"a~b":{"c/d~":[1,2,1e-16384]}}"#, "/a~0b/c~1d~0/2", "16383 digits after"),
            (
                r#"{"location":{"type":"GeoProperty","value":{"type":"Point","coordinates":[1e200000,2]}}}"#,
                "/location/value/coordinates/0",
                "131072 digits before",
            ),
            (r#"[{"id":"x"},{"n":[[1e140000]]}]"#, "/1/n/0/0", "131072 digits before"),
        ] {
            let refused = text_refusal(text).unwrap_or_else(|| panic!("{text}: accepted"));
            pretty_assertions::assert_eq!(
                refused,
                format!(
                    "the JSON number at {location} is refused: the number needs more than {limit} the decimal point; Andromeda stores numbers of at most 131072 digits before and 16383 digits after it"
                ),
                "{text}"
            );
        }
    }

    /// A text that is not JSON is refused for its syntax even when a number
    /// before the fault is out of range, as a parse of it would refuse it.
    #[test]
    fn a_syntax_error_after_an_out_of_range_number_is_a_syntax_error() {
        assert!(matches!(admit_json_text(b"[1e-16384,"), Err(JsonTextRefusal::Syntax(_))));
        assert!(matches!(admit_json_text(b"[1] x"), Err(JsonTextRefusal::Syntax(_))));
    }

    #[test]
    fn an_object_whose_first_member_is_named_like_a_number_token_is_read_as_one() {
        assert!(admit_json_text(br#"{"v":{"$serde_json::private::Number":"5"}}"#).is_ok());
    }

    /// The location of the repeated member of `text`, when it has one.
    fn repeated_member(text: &str) -> Option<String> {
        match admit_json_text(text.as_bytes()) {
            Err(JsonTextRefusal::DuplicateMember { path }) => Some(path.to_string()),
            Ok(()) => None,
            Err(other) => panic!("{text}: unexpected refusal {other}"),
        }
    }

    /// RFC 8259 section 4: a member named twice is refused at the repeated
    /// member, at the top level, below an attribute, inside a JSON value, in an
    /// array element, in a subscription and in a batch element.
    #[test]
    fn a_member_named_twice_is_refused_where_it_is_repeated() {
        for (text, location) in [
            (r#"{"id":"urn:ngsi-ld:T:1","id":"urn:ngsi-ld:T:2"}"#, "/id"),
            (
                r#"{"id":"urn:ngsi-ld:T:1","speed":{"type":"Property","value":1},"speed":{"type":"Property","value":2}}"#,
                "/speed",
            ),
            (r#"{"speed":{"type":"Property","value":1,"value":2}}"#, "/speed/value"),
            (r#"{"speed":{"type":"Property","value":{"a":1,"b":{"c":1,"c":2}}}}"#, "/speed/value/b/c"),
            (r#"{"n":{"type":"Property","value":[{"a":1},{"a":1,"a":2}]}}"#, "/n/value/1/a"),
            (
                r#"{"type":"Subscription","notification":{"endpoint":{"uri":"http://a"},"endpoint":{"uri":"http://b"}}}"#,
                "/notification/endpoint",
            ),
            (r#"[{"id":"urn:ngsi-ld:T:1"},{"id":"urn:ngsi-ld:T:2","type":"T","type":"U"}]"#, "/1/type"),
            (r#"{"a~b/c":1,"a~b/c":2}"#, "/a~0b~1c"),
        ] {
            pretty_assertions::assert_eq!(repeated_member(text).as_deref(), Some(location), "{text}");
        }
    }

    /// A name spelled with an escape is the same name as its plain spelling.
    #[test]
    fn a_member_repeated_under_an_escaped_spelling_is_refused() {
        pretty_assertions::assert_eq!(repeated_member(r#"{"a":1,"a":2}"#).as_deref(), Some("/a"));
    }

    /// The same name in two different objects, or once per array element, is no
    /// repeat.
    #[test]
    fn the_same_name_in_different_objects_is_no_repeat() {
        for text in [
            r#"{"a":{"v":1},"b":{"v":2}}"#,
            r#"[{"id":"x"},{"id":"y"}]"#,
            r#"{"a":{"v":1,"w":{"v":2}}}"#,
            r#"{"@context":"c","id":"x","type":"T","n":{"type":"Property","value":1}}"#,
            "{}",
        ] {
            assert!(repeated_member(text).is_none(), "{text}");
        }
    }

    /// The refusal names the member and where it stands.
    #[test]
    fn the_refusal_names_the_repeated_member() {
        let refused = admit_json_text(br#"{"speed":{"value":1,"value":2}}"#).expect_err("a repeat");
        assert!(refused.to_string().contains("/speed/value"), "{refused}");
        assert!(refused.to_string().contains("named twice"), "{refused}");
    }

    /// A number out of range met before the repeat is the first refusal, as the
    /// walk reports in document order.
    #[test]
    fn the_first_refusal_in_document_order_wins() {
        assert!(matches!(admit_json_text(br#"{"a":1e-16384,"b":1,"b":2}"#), Err(JsonTextRefusal::OutOfRange(_))));
        assert!(matches!(
            admit_json_text(br#"{"b":1,"b":2,"a":1e-16384}"#),
            Err(JsonTextRefusal::DuplicateMember { .. })
        ));
    }

    /// A member named with the reader's reserved prefix anywhere but in its
    /// own spelling of a number is refused at its location.
    #[test]
    fn a_member_named_with_the_readers_reserved_prefix_is_refused_where_it_stands() {
        for (text, location) in [
            (r#"{"v":{"b":1,"$serde_json::private::Number":"5"}}"#, "/v/$serde_json::private::Number"),
            (r#"[{"$serde_json::private::RawValue":"[1]"}]"#, "/0/$serde_json::private::RawValue"),
        ] {
            match admit_json_text(text.as_bytes()) {
                Err(JsonTextRefusal::ReservedMemberName { path }) => pretty_assertions::assert_eq!(path.to_string(), location, "{text}"),
                other => panic!("{text}: expected a reserved member name, got {other:?}"),
            }
        }
    }
}
