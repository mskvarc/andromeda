//! The keys a clause 4.9 `q` operand reads: every key a stored value equal to
//! it carries at the operand's name path.
//!
//! A stored time value carries the key of its value whichever spelling it was
//! written in ([`string_keys`](crate::sub_attribute_key::string_keys::string_keys)),
//! so a temporal operand reads that one key and leaves the spellings to the
//! recheck.

use crate::sub_attribute_key::{
    key::{KeyedValue, SubAttributeKey, TextKind},
    path_hash::PathHash,
};
use andromeda_ngsi_ld_lexicon::time::{date::NgsiLdDate, time_of_day::NgsiLdTime};
use andromeda_ngsi_ld_query::filter::scalar::Scalar;

/// The keys `scalar` reads at `path`.
///
/// A String or a URI operand may equal a String or a Relationship object,
/// which are keyed under two kinds, so it reads both keys. Every other operand
/// reads one.
#[must_use]
pub fn operand_keys(path: PathHash, scalar: &Scalar) -> Vec<SubAttributeKey> {
    let key = |value: KeyedValue<'_>| SubAttributeKey::of(path, value);
    match scalar {
        Scalar::Number(number) => vec![key(KeyedValue::Number(number.decimal()))],
        Scalar::Bool(flag) => vec![key(KeyedValue::Boolean(*flag))],
        Scalar::Str(text) | Scalar::Uri(text) => vec![key(KeyedValue::Text(text, TextKind::String)), key(KeyedValue::Text(text, TextKind::Uri))],
        Scalar::DateTime(instant) => vec![key(KeyedValue::Instant(*instant))],
        Scalar::Date(date) => vec![key(KeyedValue::Text(&NgsiLdDate::from(*date).to_string(), TextKind::String))],
        Scalar::Time(time) => vec![key(KeyedValue::Text(&NgsiLdTime::normalise(*time).to_string(), TextKind::String))],
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        sub_attribute_key::{
            key::{KeyedValue, SubAttributeKey, TextKind},
            operand_keys::operand_keys,
            path_hash::PathHash,
        },
    };
    use andromeda_ngsi_ld_query::filter::scalar::Scalar;
    use pretty_assertions::assert_eq;

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
}
