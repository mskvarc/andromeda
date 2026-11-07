//! Counting the values of a JSON literal a Property carries, without the
//! request's context.
//!
//! The count is a lower bound of what the document census counts for the
//! literal, whatever the context expands it under: a scalar is one value, an
//! array is the values of its elements, a non-empty object is one value plus the
//! values of its members that are no keyword. A keyword member (`@value`,
//! `@id`, `@type`, ...) is not descended into, because the census counts a
//! value object or a node once however many keywords it holds. The count is
//! taken from the text as the reader streams it, so nothing is built and a
//! literal far over the limit is refused at the first value past it.

use crate::error::ExpansionError;
use andromeda_ngsi_ld_number::json_value::value_census::{MemberName, ValueTally};
use andromeda_ngsi_ld_representation::payload_budget::PayloadBudgetExceeded;
use serde::de::{DeserializeSeed, Deserializer, Error as DeserializeError, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Deserializer as JsonReader;
use std::fmt;

/// Count the values of the JSON `text` into `tally`.
///
/// # Errors
///
/// Returns [`PayloadBudgetExceeded::EntityJsonValues`] once the tally passes
/// its limit, and [`ExpansionError::CompactDocument`] for a text that is no
/// JSON.
pub(crate) fn count_literal(tally: &ValueTally, text: &str) -> Result<(), ExpansionError> {
    let mut reader = JsonReader::from_str(text);
    let read = LiteralValues { tally }.deserialize(&mut reader).and_then(|()| reader.end());
    match read {
        Ok(()) => Ok(()),
        Err(_aborted) if tally.exceeded() => Err(ExpansionError::PayloadBudget(PayloadBudgetExceeded::EntityJsonValues { limit: tally.limit() })),
        Err(error) => Err(ExpansionError::CompactDocument(error)),
    }
}

/// Counts one JSON value of a literal and the values below it.
#[derive(Clone, Copy)]
struct LiteralValues<'t> {
    /// The tally the values are counted into.
    tally: &'t ValueTally,
}

impl<'de> DeserializeSeed<'de> for LiteralValues<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for LiteralValues<'_> {
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
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<(), A::Error> {
        while items.next_element_seed(self)?.is_some() {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<(), A::Error> {
        let Some(first) = members.next_key_seed(MemberKindSeed)? else {
            return Ok(());
        };
        if first == MemberKind::Number {
            members.next_value::<IgnoredAny>()?;
            return self.tally.count();
        }
        self.tally.count()?;
        let mut kind = first;
        loop {
            match kind {
                MemberKind::Term => members.next_value_seed(self)?,
                MemberKind::Keyword | MemberKind::Number => {
                    members.next_value::<IgnoredAny>()?;
                }
            }
            match members.next_key_seed(MemberKindSeed)? {
                Some(next) => kind = next,
                None => return Ok(()),
            }
        }
    }
}

/// What the name of an object member says about the member.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MemberKind {
    /// The name the reader spells a number token with: the object is a number.
    Number,
    /// A JSON-LD keyword, whose value the census does not count by value.
    Keyword,
    /// Any other name: a member whose values the census counts.
    Term,
}

/// Reads one member name without keeping it, telling what kind of member it
/// names.
#[derive(Clone, Copy)]
struct MemberKindSeed;

impl<'de> DeserializeSeed<'de> for MemberKindSeed {
    type Value = MemberKind;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<MemberKind, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for MemberKindSeed {
    type Value = MemberKind;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a member name")
    }

    fn visit_str<E: DeserializeError>(self, name: &str) -> Result<MemberKind, E> {
        Ok(match MemberName::of(name) {
            MemberName::NumberToken => MemberKind::Number,
            MemberName::Member if name.starts_with('@') => MemberKind::Keyword,
            MemberName::Member => MemberKind::Term,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::ExpansionError, literal_values::count_literal};
    use andromeda_ngsi_ld_number::json_value::value_census::{JsonValueLimit, ValueTally};
    use andromeda_ngsi_ld_representation::payload_budget::PayloadBudgetExceeded;
    use std::num::NonZeroUsize;

    fn counted(text: &str) -> usize {
        let tally = ValueTally::new(JsonValueLimit::new(NonZeroUsize::MAX));
        count_literal(&tally, text).expect("a countable literal");
        tally.counted().get()
    }

    #[test]
    fn a_scalar_is_one_value_and_null_is_none() {
        for (text, values) in [("1", 1), ("\"a\"", 1), ("true", 1), ("1.5", 1), ("null", 0)] {
            pretty_assertions::assert_eq!(counted(text), values, "{text}");
        }
    }

    #[test]
    fn an_array_is_the_values_of_its_elements() {
        pretty_assertions::assert_eq!(counted("[1,2,[3,4],null]"), 4);
        pretty_assertions::assert_eq!(counted("[]"), 0);
    }

    #[test]
    fn an_object_is_one_value_plus_the_values_of_its_terms() {
        pretty_assertions::assert_eq!(counted(r#"{"a":[1,2],"b":"x"}"#), 4);
        pretty_assertions::assert_eq!(counted("{}"), 0);
    }

    #[test]
    fn a_keyword_member_is_not_counted_by_value() {
        pretty_assertions::assert_eq!(counted(r#"{"@value":5,"@type":"T"}"#), 1);
        pretty_assertions::assert_eq!(counted(r#"{"@id":"urn:a"}"#), 1);
        pretty_assertions::assert_eq!(counted(r#"{"@list":[1,2,3],"a":[1]}"#), 2);
    }

    #[test]
    fn a_number_the_reader_spells_as_an_object_is_one_value() {
        pretty_assertions::assert_eq!(counted("123456789012345678901234567890"), 1);
        pretty_assertions::assert_eq!(counted("[123456789012345678901234567890,1.0000000000000000000001]"), 2);
    }

    #[test]
    fn a_literal_over_the_limit_is_refused_at_the_first_value_past_it() {
        let tally = ValueTally::new(JsonValueLimit::new(NonZeroUsize::new(3).expect("a positive limit")));
        let refused = count_literal(&tally, "[1,2,3,4,5,6]").expect_err("six values exceed three");
        assert!(
            matches!(refused, ExpansionError::PayloadBudget(PayloadBudgetExceeded::EntityJsonValues { .. })),
            "{refused:?}"
        );
        pretty_assertions::assert_eq!(tally.counted().get(), 4);
    }

    #[test]
    fn a_literal_at_the_limit_is_admitted() {
        let tally = ValueTally::new(JsonValueLimit::new(NonZeroUsize::new(3).expect("a positive limit")));
        count_literal(&tally, "[1,2,3]").expect("three values within three");
    }

    #[test]
    fn a_text_that_is_no_json_is_a_compact_document_failure() {
        let tally = ValueTally::new(JsonValueLimit::new(NonZeroUsize::MAX));
        let refused = count_literal(&tally, "[1,").expect_err("truncated");
        assert!(matches!(refused, ExpansionError::CompactDocument(_)), "{refused:?}");
    }
}
