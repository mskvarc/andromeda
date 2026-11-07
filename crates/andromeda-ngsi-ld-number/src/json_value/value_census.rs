//! Counting the JSON values of a text before it becomes a tree.
//!
//! A parsed JSON document costs memory per value, not per byte: a 28 MB body of
//! short numbers is several million values, and every later representation of
//! it -- the parsed tree, its typed decode, its JSON-LD expansion -- multiplies
//! that count. IETF RFC 8259 section 9 lets an implementation "set limits on
//! the size of texts that it accepts", and the census is how such a limit is
//! taken without building anything: it drives `serde_json`'s own streaming
//! reader through a visitor that only counts, and stops the moment the count
//! passes the limit.
//!
//! The reader is the one every body is parsed with afterwards, so the census
//! refuses every text the parse refuses for its syntax -- malformed JSON,
//! trailing content, nesting past the reader's recursion limit -- and counts
//! every number once whatever its spelling, although with `arbitrary_precision`
//! the reader hands a number that is not a 64-bit integer to a visitor as a
//! one-member map holding its token. An object whose first member is spelled
//! like that token is counted as the one number the reader takes it for,
//! without the census checking the token's text, which the parse afterwards
//! does.
//!
//! The pieces of the walk -- the running [`ValueTally`], the
//! [`ValueCensusSeed`] counting one value and everything below it, and the
//! [`MemberNameSeed`] telling a number token from an object -- are public, so
//! a census that counts part of a text differently, such as the positions of a
//! geometry, counts the rest of it exactly as this one does.

use crate::json_value::reader_private_members::SERDE_JSON_PRIVATE_NUMBER_MEMBER;
use derive_more::Display;
use serde::de::{DeserializeSeed, Deserializer, Error as DeserializeError, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Deserializer as JsonReader;
use std::{cell::Cell, fmt, num::NonZeroUsize};
use thiserror::Error;

/// The most JSON values one text may hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Display)]
#[display("{_0}")]
pub struct JsonValueLimit(NonZeroUsize);

impl JsonValueLimit {
    /// A limit of `values` JSON values.
    #[must_use]
    pub const fn new(values: NonZeroUsize) -> JsonValueLimit {
        JsonValueLimit(values)
    }

    /// The number of values the limit admits.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get()
    }
}

/// How many JSON values a text holds: every scalar, array and object, each
/// counted once, member names not counted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Display)]
#[display("{_0}")]
pub struct JsonValueCount(usize);

impl JsonValueCount {
    /// A count of `values` JSON values.
    #[must_use]
    pub const fn new(values: usize) -> JsonValueCount {
        JsonValueCount(values)
    }

    /// The number of values counted.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// Why a text's values could not be counted within a limit.
#[derive(Debug, Error)]
pub enum ValueCensusError {
    /// The text holds more JSON values than the limit admits; counting stopped
    /// at the first value past it.
    #[error("the JSON text holds more than {limit} JSON values")]
    OverLimit {
        /// The limit the text exceeds.
        limit: JsonValueLimit,
    },
    /// The text is not one well-formed JSON value as `serde_json`'s reader
    /// reads it.
    #[error(transparent)]
    Syntax(#[from] serde_json::Error),
}

/// Count the JSON values of `text`, refusing it as soon as the count passes
/// `limit`.
///
/// No value is built: the count is the only state the walk keeps.
///
/// # Errors
///
/// Returns [`ValueCensusError::OverLimit`] once the text is found to hold more
/// than `limit` values, and [`ValueCensusError::Syntax`] for a text the
/// streaming reader refuses, trailing content included.
pub fn count_values_within(text: &[u8], limit: JsonValueLimit) -> Result<JsonValueCount, ValueCensusError> {
    let tally = ValueTally::new(limit);
    let mut reader = JsonReader::from_slice(text);
    let read = ValueCensusSeed::new(&tally).deserialize(&mut reader).and_then(|()| reader.end());
    match read {
        Ok(()) => Ok(tally.counted()),
        Err(_aborted) if tally.exceeded() => Err(ValueCensusError::OverLimit { limit }),
        Err(error) => Err(ValueCensusError::Syntax(error)),
    }
}

/// One value more than a census's limit admits was counted.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("the JSON text holds more than {limit} JSON values")]
pub struct ValueLimitExceeded {
    /// The limit the count passed.
    pub limit: JsonValueLimit,
}

/// The running count of one census, shared by every seed that counts into it.
#[derive(Debug)]
pub struct ValueTally {
    /// Values counted so far.
    counted: Cell<usize>,
    /// The most values the text may hold.
    limit: JsonValueLimit,
    /// Whether a count passed the limit, which tells a walk aborted for it
    /// apart from one the reader aborted for a syntax error.
    exceeded: Cell<bool>,
}

impl ValueTally {
    /// A tally of no values under `limit`.
    #[must_use]
    pub const fn new(limit: JsonValueLimit) -> ValueTally {
        ValueTally {
            counted: Cell::new(0),
            limit,
            exceeded: Cell::new(false),
        }
    }

    /// Count one value.
    ///
    /// # Errors
    ///
    /// Returns [`ValueLimitExceeded`] once the count passes the limit, and on
    /// every count after it.
    pub fn admit_one(&self) -> Result<(), ValueLimitExceeded> {
        self.admit(1)
    }

    /// Count `values` values at once, as `values` calls of
    /// [`ValueTally::admit_one`] would.
    ///
    /// # Errors
    ///
    /// Returns [`ValueLimitExceeded`] once the count passes the limit, and on
    /// every count after it.
    pub fn admit(&self, values: usize) -> Result<(), ValueLimitExceeded> {
        let counted = self.counted.get().saturating_add(values);
        self.counted.set(counted);
        if counted > self.limit.get() {
            self.exceeded.set(true);
            return Err(ValueLimitExceeded { limit: self.limit });
        }
        Ok(())
    }

    /// Count one value inside a streamed walk, aborting the walk once the
    /// count passes the limit.
    ///
    /// # Errors
    ///
    /// Returns the reader's error carrying [`ValueLimitExceeded`] once the
    /// count passes the limit; [`ValueTally::exceeded`] tells it apart from a
    /// syntax error afterwards.
    pub fn count<E: DeserializeError>(&self) -> Result<(), E> {
        self.admit_one().map_err(E::custom)
    }

    /// The values counted so far.
    #[must_use]
    pub const fn counted(&self) -> JsonValueCount {
        JsonValueCount(self.counted.get())
    }

    /// Whether a count passed the limit.
    #[must_use]
    pub const fn exceeded(&self) -> bool {
        self.exceeded.get()
    }

    /// The limit the tally counts against.
    #[must_use]
    pub const fn limit(&self) -> JsonValueLimit {
        self.limit
    }
}

/// Counts one JSON value and everything below it into a [`ValueTally`].
#[derive(Clone, Copy, Debug)]
pub struct ValueCensusSeed<'a> {
    /// The census this value is counted into.
    tally: &'a ValueTally,
}

impl ValueCensusSeed<'_> {
    /// A seed counting into `tally`.
    #[must_use]
    pub const fn new(tally: &ValueTally) -> ValueCensusSeed<'_> {
        ValueCensusSeed { tally }
    }

    /// Count an object whose first member name has been read and was no
    /// number token: the object, the value of that first member, and every
    /// further member's value.
    ///
    /// # Errors
    ///
    /// Returns the reader's error, or the error carrying
    /// [`ValueLimitExceeded`] once the count passes the limit.
    pub fn count_object<'de, A: MapAccess<'de>>(self, mut members: A) -> Result<(), A::Error> {
        self.tally.count()?;
        members.next_value_seed(self)?;
        while members.next_key::<IgnoredAny>()?.is_some() {
            members.next_value_seed(self)?;
        }
        Ok(())
    }
}

impl<'de> DeserializeSeed<'de> for ValueCensusSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for ValueCensusSeed<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E: DeserializeError>(self, _value: bool) -> Result<(), E> {
        self.tally.count()
    }

    fn visit_i64<E: DeserializeError>(self, _value: i64) -> Result<(), E> {
        self.tally.count()
    }

    fn visit_u64<E: DeserializeError>(self, _value: u64) -> Result<(), E> {
        self.tally.count()
    }

    fn visit_f64<E: DeserializeError>(self, _value: f64) -> Result<(), E> {
        self.tally.count()
    }

    fn visit_str<E: DeserializeError>(self, _value: &str) -> Result<(), E> {
        self.tally.count()
    }

    fn visit_unit<E: DeserializeError>(self) -> Result<(), E> {
        self.tally.count()
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<(), A::Error> {
        self.tally.count()?;
        while items.next_element_seed(self)?.is_some() {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<(), A::Error> {
        match members.next_key_seed(MemberNameSeed)? {
            None => self.tally.count(),
            // The reader's spelling of one number, not an object.
            Some(MemberName::NumberToken) => {
                members.next_value::<IgnoredAny>()?;
                self.tally.count()
            }
            Some(MemberName::Member) => self.count_object(members),
        }
    }
}

/// What the first member name of an object tells about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberName {
    /// The private member name `serde_json`'s reader spells a number token
    /// with.
    NumberToken,
    /// Any other name: the object is an object.
    Member,
}

impl MemberName {
    /// What `name`, read as the first member name of an object, tells about
    /// the object.
    #[must_use]
    pub fn of(name: &str) -> MemberName {
        if name == SERDE_JSON_PRIVATE_NUMBER_MEMBER {
            MemberName::NumberToken
        } else {
            MemberName::Member
        }
    }
}

/// Reads one member name without keeping it, telling the reader's spelling of
/// a number token from the name of an object member.
#[derive(Clone, Copy, Debug)]
pub struct MemberNameSeed;

impl<'de> DeserializeSeed<'de> for MemberNameSeed {
    type Value = MemberName;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<MemberName, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for MemberNameSeed {
    type Value = MemberName;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a member name")
    }

    fn visit_str<E: DeserializeError>(self, name: &str) -> Result<MemberName, E> {
        Ok(MemberName::of(name))
    }
}

#[cfg(test)]
mod tests {
    use crate::json_value::value_census::{JsonValueCount, JsonValueLimit, ValueCensusError, ValueLimitExceeded, ValueTally, count_values_within};
    use serde_json::Value;
    use std::num::NonZeroUsize;

    /// A limit of `values`.
    fn limit(values: usize) -> JsonValueLimit {
        JsonValueLimit::new(NonZeroUsize::new(values).expect("a positive test limit"))
    }

    /// The count of `text` under a limit it cannot reach.
    fn counted(text: &[u8]) -> usize {
        count_values_within(text, limit(usize::MAX)).expect("a countable text").get()
    }

    #[test]
    fn a_scalar_document_is_one_value() {
        pretty_assertions::assert_eq!(count_values_within(b"42", limit(10)).expect("countable"), JsonValueCount::new(1));
        for scalar in [&b"true"[..], b"null", b"\"text\"", b"-7", b"0.5"] {
            pretty_assertions::assert_eq!(counted(scalar), 1, "{}", String::from_utf8_lossy(scalar));
        }
    }

    #[test]
    fn an_array_counts_itself_and_each_element() {
        pretty_assertions::assert_eq!(counted(b"[1,2,3]"), 4);
        pretty_assertions::assert_eq!(counted(b"[]"), 1);
    }

    #[test]
    fn an_object_counts_itself_and_each_member_value() {
        pretty_assertions::assert_eq!(counted(b"{\"a\":1,\"b\":[2,3]}"), 5);
        pretty_assertions::assert_eq!(counted(b"{}"), 1);
    }

    /// With `arbitrary_precision` the reader spells a number that is not a
    /// 64-bit integer as a one-member map; it is still one value.
    #[test]
    fn a_decimal_counts_once_like_an_integer() {
        pretty_assertions::assert_eq!(counted(b"[0.1,1e-400,7]"), 4);
        pretty_assertions::assert_eq!(counted(b"{\"n\":18446744073709551616}"), 2);
    }

    /// A client member that happens to carry the private name is an object
    /// to the reader all the same, and counted as the number it reads as.
    #[test]
    fn a_member_spelled_like_the_private_number_token_is_counted_as_the_reader_reads_it() {
        // First, the token makes the object one number; anywhere else, the
        // object is an object of members.
        pretty_assertions::assert_eq!(counted(b"{\"$serde_json::private::Number\":\"5\"}"), 1);
        pretty_assertions::assert_eq!(counted(b"{\"a\":1,\"$serde_json::private::Number\":\"5\"}"), 3);
        let parsed: Value = serde_json::from_slice(b"{\"$serde_json::private::Number\":\"5\"}").expect("the reader reads the token");
        assert!(parsed.is_number(), "{parsed}");
    }

    #[test]
    fn the_census_stops_at_the_first_value_past_the_limit() {
        let refused = count_values_within(b"[1,2,3,4]", limit(3)).expect_err("four values exceed three");
        assert!(
            matches!(refused, ValueCensusError::OverLimit { limit: refused_at } if refused_at == limit(3)),
            "{refused:?}"
        );
        pretty_assertions::assert_eq!(refused.to_string(), "the JSON text holds more than 3 JSON values");
    }

    /// The count stops before a later syntax error is reached: what decides is
    /// the first of the two the reader meets.
    #[test]
    fn a_text_past_the_limit_is_refused_before_its_later_syntax_error() {
        let refused = count_values_within(b"[1,2,3,4,", limit(3)).expect_err("refused");
        assert!(matches!(refused, ValueCensusError::OverLimit { .. }), "{refused:?}");
    }

    #[test]
    fn a_document_exactly_at_the_limit_is_admitted() {
        pretty_assertions::assert_eq!(count_values_within(b"[1,2]", limit(3)).expect("admitted").get(), 3);
    }

    #[test]
    fn malformed_json_is_a_syntax_error() {
        for text in [&b"[1,"[..], b"{\"a\"}", b"", b"nul"] {
            let refused = count_values_within(text, limit(10)).expect_err("malformed");
            assert!(matches!(refused, ValueCensusError::Syntax(_)), "{}: {refused:?}", String::from_utf8_lossy(text));
        }
    }

    /// The census refuses exactly the nesting `serde_json::from_slice` refuses.
    #[test]
    fn nesting_the_reader_refuses_is_refused_by_the_census() {
        let nested = |depth: usize| format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        let accepted = nested(127);
        assert!(serde_json::from_str::<Value>(&accepted).is_ok());
        pretty_assertions::assert_eq!(counted(accepted.as_bytes()), 127);
        let refused = nested(129);
        assert!(serde_json::from_str::<Value>(&refused).is_err());
        let refusal = count_values_within(refused.as_bytes(), limit(usize::MAX)).expect_err("too deep");
        assert!(matches!(refusal, ValueCensusError::Syntax(_)), "{refusal:?}");
    }

    /// The deepest nesting the reader accepts is 127 containers; the census
    /// accepts it and refuses the 128th exactly where the reader does.
    #[test]
    fn nesting_at_the_reader_boundary_is_accepted_and_one_more_is_refused() {
        let nested = |depth: usize| format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        let deepest = nested(127);
        assert!(serde_json::from_str::<Value>(&deepest).is_ok());
        pretty_assertions::assert_eq!(counted(deepest.as_bytes()), 127);
        let past = nested(128);
        assert!(serde_json::from_str::<Value>(&past).is_err());
        let refusal = count_values_within(past.as_bytes(), limit(usize::MAX)).expect_err("one past the reader's depth");
        assert!(matches!(refusal, ValueCensusError::Syntax(_)), "{refusal:?}");
    }

    #[test]
    fn a_tally_refuses_every_count_past_its_limit() {
        let tally = ValueTally::new(limit(2));
        assert!(tally.admit_one().is_ok() && tally.admit_one().is_ok());
        pretty_assertions::assert_eq!(tally.admit_one(), Err(ValueLimitExceeded { limit: limit(2) }));
        assert!(tally.exceeded());
        pretty_assertions::assert_eq!(tally.counted().get(), 3);
    }

    #[test]
    fn a_tally_counts_several_values_at_once_up_to_its_limit() {
        let tally = ValueTally::new(limit(5));
        assert!(tally.admit(3).is_ok() && tally.admit(2).is_ok());
        assert!(!tally.exceeded());
        pretty_assertions::assert_eq!(tally.admit(1), Err(ValueLimitExceeded { limit: limit(5) }));
        assert!(tally.exceeded());
    }

    #[test]
    fn a_tally_refuses_a_batch_that_passes_the_limit_and_does_not_overflow() {
        let tally = ValueTally::new(limit(5));
        pretty_assertions::assert_eq!(tally.admit(usize::MAX), Err(ValueLimitExceeded { limit: limit(5) }));
        pretty_assertions::assert_eq!(tally.admit(usize::MAX), Err(ValueLimitExceeded { limit: limit(5) }));
    }

    #[test]
    fn trailing_content_is_a_syntax_error() {
        let refused = count_values_within(b"1 2", limit(10)).expect_err("trailing content");
        assert!(matches!(refused, ValueCensusError::Syntax(_)), "{refused:?}");
    }

    #[test]
    fn strings_and_keys_count_as_one_value_each_whatever_their_length() {
        let long = format!("\"{}\"", "x".repeat(1 << 20));
        pretty_assertions::assert_eq!(counted(long.as_bytes()), 1);
        let keyed = format!("{{\"{}\":1}}", "k".repeat(1 << 16));
        pretty_assertions::assert_eq!(counted(keyed.as_bytes()), 2);
    }

    /// The census agrees with a count taken over the parsed tree.
    #[test]
    fn the_census_matches_a_count_of_the_parsed_tree() {
        fn tree_count(value: &Value) -> usize {
            1 + match value {
                Value::Array(items) => items.iter().map(tree_count).sum(),
                Value::Object(members) => members.values().map(tree_count).sum(),
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => 0,
            }
        }
        let text = br#"{"id":"urn:ngsi-ld:T:1","type":"T","location":{"type":"GeoProperty","value":{"type":"Polygon","coordinates":[[[0.5,1.25],[2,3],[4.5,-6.75],[0.5,1.25]]]}},"n":{"type":"Property","value":[1,2.5,null,true,"x",{"a":[]}]}}"#;
        let tree: Value = serde_json::from_slice(text).expect("valid JSON");
        pretty_assertions::assert_eq!(counted(text), tree_count(&tree));
    }
}
