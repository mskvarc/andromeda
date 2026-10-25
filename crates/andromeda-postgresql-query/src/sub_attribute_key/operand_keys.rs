//! The keys a clause 4.9 `q` operand reads: every key a stored value equal to
//! it carries at the operand's name path.
//!
//! A value declared as a time value by its own datatype carries the key of its
//! kind. A plain member of an array the attribute's `valueType` declares is
//! keyed as the String text it is held as, since the key of a value is
//! computed without its attribute's `valueType`, so a temporal operand also
//! reads the String key of its served spelling and leaves the declaration to
//! the recheck.

use crate::sub_attribute_key::{
    key::{KeyedValue, SubAttributeKey, TextKind},
    path_hash::PathHash,
};
use andromeda_ngsi_ld_lexicon::time::{
    date::NgsiLdDate,
    rendering::{instant_text, time_of_day_text},
    time_of_day::NgsiLdTime,
};
use andromeda_ngsi_ld_query::filter::scalar::Scalar;

/// The keys `scalar` reads at `path`.
///
/// A String or a URI operand may equal a String or a Relationship object,
/// which are keyed under two kinds, so it reads both keys. A time operand reads
/// the key of its kind and the String key of its served spelling. A number or
/// a boolean reads one.
#[must_use]
pub fn operand_keys(path: PathHash, scalar: &Scalar) -> Vec<SubAttributeKey> {
    let key = |value: KeyedValue<'_>| SubAttributeKey::of(path, value);
    match scalar {
        Scalar::Number(number) => vec![key(KeyedValue::Number(number.decimal()))],
        Scalar::Bool(flag) => vec![key(KeyedValue::Boolean(*flag))],
        Scalar::Str(text) | Scalar::Uri(text) => vec![key(KeyedValue::Text(text, TextKind::String)), key(KeyedValue::Text(text, TextKind::Uri))],
        Scalar::DateTime(instant) => vec![
            key(KeyedValue::Instant(*instant)),
            key(KeyedValue::Text(&instant_text(*instant), TextKind::String)),
        ],
        Scalar::Date(date) => {
            let text = NgsiLdDate::from(*date).to_string();
            vec![key(KeyedValue::Text(&text, TextKind::Date)), key(KeyedValue::Text(&text, TextKind::String))]
        }
        Scalar::Time(time) => vec![
            key(KeyedValue::Text(&NgsiLdTime::normalise(*time).to_string(), TextKind::Time)),
            key(KeyedValue::Text(&time_of_day_text(*time), TextKind::String)),
        ],
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        sub_attribute_key::{
            key::{KeyedValue, SubAttributeKey, TextKind},
            operand_keys::operand_keys,
            path_hash::PathHash,
        },
    };
    use andromeda_ngsi_ld_query::filter::scalar::{Scalar, parse_date, parse_time};
    use pretty_assertions::{assert_eq, assert_ne};

    fn path() -> PathHash {
        PathHash::of(&[AttributeNameKey::from(2), AttributeNameKey::from(7)])
    }

    #[test]
    fn a_number_operand_reads_the_key_of_its_value() {
        let one = operand_keys(path(), &Scalar::Number("1".parse().expect("a number")));
        assert_eq!(operand_keys(path(), &Scalar::Number("1.00".parse().expect("a number"))), one);
        assert_eq!(one.len(), 1);
    }

    #[test]
    fn a_text_operand_reads_the_string_and_the_uri_key() {
        let expected = vec![
            SubAttributeKey::of(path(), KeyedValue::Text("urn:a", TextKind::String)),
            SubAttributeKey::of(path(), KeyedValue::Text("urn:a", TextKind::Uri)),
        ];
        assert_eq!(operand_keys(path(), &Scalar::Str("urn:a".to_owned())), expected);
        assert_eq!(operand_keys(path(), &Scalar::Uri("urn:a".to_owned())), expected);
    }

    /// A `Time` reads the key of its kind over its fixed-width encoding, and
    /// the String key a `valueType`-declared member is held under.
    #[test]
    fn a_time_operand_reads_its_kind_key_and_the_string_key_of_its_served_spelling() {
        let time = parse_time("12:00:00,5Z").expect("a time");
        assert_eq!(
            operand_keys(path(), &Scalar::Time(time)),
            vec![
                SubAttributeKey::of(path(), KeyedValue::Text("12:00:00.500000Z", TextKind::Time)),
                SubAttributeKey::of(path(), KeyedValue::Text("12:00:00.500Z", TextKind::String)),
            ]
        );
    }

    #[test]
    fn a_date_operand_reads_its_kind_key_and_its_string_key() {
        let date = parse_date("2024-01-01").expect("a date");
        assert_eq!(
            operand_keys(path(), &Scalar::Date(date)),
            vec![
                SubAttributeKey::of(path(), KeyedValue::Text("2024-01-01", TextKind::Date)),
                SubAttributeKey::of(path(), KeyedValue::Text("2024-01-01", TextKind::String)),
            ]
        );
    }

    #[test]
    fn the_kind_salts_a_date_key_apart_from_its_string_key() {
        assert_ne!(
            SubAttributeKey::of(path(), KeyedValue::Text("2024-01-01", TextKind::Date)),
            SubAttributeKey::of(path(), KeyedValue::Text("2024-01-01", TextKind::String))
        );
    }
}
