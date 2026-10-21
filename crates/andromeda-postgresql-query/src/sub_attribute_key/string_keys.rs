//! The keys of one stored string.
//!
//! A string is held as written, and clause 4.6.3 admits several spellings of
//! one time value: a fraction of no digits or up to six, with the decimal point
//! or the comma. A string written in the `DateTime` form is therefore also
//! keyed by its instant, and one written in the `Time` form by its fixed-width
//! spelling, which is the key a `q` operand of that value reads whichever
//! spelling either side was written in. A `Date` has one spelling, its text.

use crate::sub_attribute_key::{
    key::{KeyedValue, SubAttributeKey, TextKind},
    path_hash::PathHash,
};
use andromeda_ngsi_ld_lexicon::time::value::{TimeValue, TimeValueForm};
use std::iter::once;

/// The keys of the String `text` at `path`: its text key, then the key of the
/// time value it spells, when it spells one under another text.
pub fn string_keys(path: PathHash, text: &str) -> impl Iterator<Item = SubAttributeKey> + use<> {
    let text_key = SubAttributeKey::of(path, KeyedValue::Text(text, TextKind::String));
    let value_key = TimeValueForm::of(text).and_then(|form| form.parse(text).ok()).and_then(|value| match value {
        TimeValue::DateTime(instant) => Some(SubAttributeKey::of(path, KeyedValue::Instant(instant.get()))),
        TimeValue::Time(time) => {
            let fixed_width = time.to_string();
            (fixed_width != text).then(|| SubAttributeKey::of(path, KeyedValue::Text(&fixed_width, TextKind::String)))
        }
        TimeValue::Date(_) => None,
    });
    once(text_key).chain(value_key)
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        sub_attribute_key::{
            key::{KeyedValue, SubAttributeKey, TextKind},
            operand_keys::operand_keys,
            path_hash::PathHash,
            string_keys::string_keys,
        },
    };
    use andromeda_ngsi_ld_lexicon::time::spellings::{instant_spellings, time_of_day_spellings};
    use andromeda_ngsi_ld_query::filter::scalar::Scalar;
    use chrono::{NaiveDate, NaiveTime, TimeZone, Utc};
    use pretty_assertions::assert_eq;

    fn path() -> PathHash {
        PathHash::of(&[AttributeNameKey::from(2), AttributeNameKey::from(7)])
    }

    fn keys(text: &str) -> Vec<SubAttributeKey> {
        string_keys(path(), text).collect()
    }

    fn text_key(text: &str) -> SubAttributeKey {
        SubAttributeKey::of(path(), KeyedValue::Text(text, TextKind::String))
    }

    /// The lexicon's spelling list is the oracle: each spelling a client may
    /// write carries the one key a `DateTime` operand of the instant reads.
    #[test]
    fn every_spelling_of_an_instant_carries_the_key_its_operand_reads() {
        let instant = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).single().expect("a valid instant");
        let operand = operand_keys(path(), &Scalar::DateTime(instant));
        assert_eq!(operand.len(), 1);
        for spelling in instant_spellings(instant) {
            assert!(keys(&spelling).contains(&operand[0]), "{spelling}");
        }
    }

    #[test]
    fn every_spelling_of_a_time_of_day_carries_its_fixed_width_key() {
        let time = NaiveTime::from_hms_milli_opt(12, 0, 0, 500).expect("a time of day");
        let fixed_width = text_key("12:00:00.500000Z");
        assert_eq!(operand_keys(path(), &Scalar::Time(time)), vec![fixed_width]);
        for spelling in time_of_day_spellings(time) {
            assert!(keys(&spelling).contains(&fixed_width), "{spelling}");
        }
    }

    #[test]
    fn the_fixed_width_spelling_is_keyed_once() {
        assert_eq!(keys("12:00:00.500000Z"), vec![text_key("12:00:00.500000Z")]);
    }

    #[test]
    fn a_string_that_only_resembles_a_time_value_keys_as_its_text() {
        for text in ["25:00:00Z", "2024-02-30T00:00:00Z", "12:00:00", "atrium"] {
            assert_eq!(keys(text), vec![text_key(text)], "{text}");
        }
    }

    #[test]
    fn a_date_keys_as_its_text_which_its_operand_reads() {
        let date = NaiveDate::from_ymd_opt(2024, 1, 1).expect("a date");
        assert_eq!(keys("2024-01-01"), vec![text_key("2024-01-01")]);
        assert_eq!(operand_keys(path(), &Scalar::Date(date)), vec![text_key("2024-01-01")]);
    }
}
