//! The equality key of one value at one name path.
//!
//! A key is XXH3-64 of the value's encoding, seeded with the path hash
//! exclusive-or the value kind code, so a value keys apart at two paths and
//! under two kinds. The encodings fix what equality means for a key:
//!
//! - a number is its normalised decimal, the digits as a signed little-endian
//!   integer followed by the exponent as a little-endian `i64`, so `1`, `1.0`
//!   and `1E+0` share a key;
//! - a text is its UTF-8;
//! - a boolean is one byte, 0 or 1;
//! - an instant is its epoch microseconds as a little-endian `i64`.

use crate::sub_attribute_key::path_hash::PathHash;
use andromeda_postgresql_codes::value_kind::ValueKind;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use xxhash_rust::xxh3::xxh3_64_with_seed;

/// The value kind a text is keyed under, which salts its key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    /// A String.
    String,
    /// A URI, the object of a Relationship.
    Uri,
    /// A value declared as a clause 4.6.3 `Date`, as its one spelling.
    Date,
    /// A value declared as a clause 4.6.3 `Time`, in its fixed-width storage
    /// encoding.
    Time,
}

/// One value a key is computed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyedValue<'a> {
    /// A number.
    Number(&'a BigDecimal),
    /// A text, held under `TextKind`.
    Text(&'a str, TextKind),
    /// A boolean.
    Boolean(bool),
    /// A value declared as a clause 4.6.3 `DateTime`, keyed at the microsecond.
    Instant(DateTime<Utc>),
}

impl KeyedValue<'_> {
    /// The value kind that salts the key.
    const fn kind(&self) -> ValueKind {
        match self {
            KeyedValue::Number(_) => ValueKind::Number,
            KeyedValue::Text(_, TextKind::String) => ValueKind::String,
            KeyedValue::Text(_, TextKind::Uri) => ValueKind::Uri,
            KeyedValue::Text(_, TextKind::Date) => ValueKind::Date,
            KeyedValue::Text(_, TextKind::Time) => ValueKind::Time,
            KeyedValue::Boolean(_) => ValueKind::Boolean,
            KeyedValue::Instant(_) => ValueKind::DateTime,
        }
    }

    /// XXH3-64 of the value's encoding, seeded with `seed`.
    fn hash(&self, seed: u64) -> u64 {
        match self {
            KeyedValue::Number(number) => {
                let (digits, exponent) = number.normalized().into_bigint_and_exponent();
                let mut encoding = digits.to_signed_bytes_le();
                encoding.extend_from_slice(&exponent.to_le_bytes());
                xxh3_64_with_seed(&encoding, seed)
            }
            KeyedValue::Text(text, _) => xxh3_64_with_seed(text.as_bytes(), seed),
            KeyedValue::Boolean(flag) => xxh3_64_with_seed(&[u8::from(*flag)], seed),
            KeyedValue::Instant(instant) => xxh3_64_with_seed(&instant.timestamp_micros().to_le_bytes(), seed),
        }
    }
}

/// The equality key of one value at one name path, as `sub_eq_keys` stores
/// it: the hash's bits read as a signed `bigint`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubAttributeKey(i64);

impl SubAttributeKey {
    /// The key of `value` at `path`.
    #[must_use]
    pub fn of(path: PathHash, value: KeyedValue<'_>) -> SubAttributeKey {
        let seed = path.get() ^ u64::from(value.kind().code().cast_unsigned());
        SubAttributeKey(value.hash(seed).cast_signed())
    }

    /// The key as the `bigint` the column stores.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
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
            path_hash::PathHash,
        },
    };
    use bigdecimal::BigDecimal;
    use chrono::{TimeZone, Utc};
    use pretty_assertions::{assert_eq, assert_ne};

    fn path() -> PathHash {
        PathHash::of(&[AttributeNameKey::from(2), AttributeNameKey::from(7)])
    }

    fn number_key(text: &str) -> SubAttributeKey {
        let number: BigDecimal = text.parse().expect("a decimal");
        SubAttributeKey::of(path(), KeyedValue::Number(&number))
    }

    #[test]
    fn every_spelling_of_one_number_shares_a_key() {
        for spelling in ["1.0", "1.00", "1E+0", "0.1e1"] {
            assert_eq!(number_key(spelling), number_key("1"), "{spelling}");
        }
        assert_eq!(number_key("0.000"), number_key("0"));
        assert_eq!(number_key("100"), number_key("1E+2"));
        assert_eq!(number_key("-2.50"), number_key("-2.5"));
    }

    #[test]
    fn two_numbers_key_apart() {
        assert_ne!(number_key("1"), number_key("2"));
        assert_ne!(number_key("1"), number_key("-1"));
        assert_ne!(number_key("1"), number_key("10"));
        assert_ne!(number_key("0.1"), number_key("1"));
    }

    #[test]
    fn the_kind_salts_a_text_key() {
        assert_ne!(
            SubAttributeKey::of(path(), KeyedValue::Text("x", TextKind::String)),
            SubAttributeKey::of(path(), KeyedValue::Text("x", TextKind::Uri))
        );
    }

    #[test]
    fn the_path_salts_every_key() {
        let other = PathHash::of(&[AttributeNameKey::from(2), AttributeNameKey::from(8)]);
        assert_ne!(
            SubAttributeKey::of(path(), KeyedValue::Boolean(true)),
            SubAttributeKey::of(other, KeyedValue::Boolean(true))
        );
    }

    /// The keys are stored, so a change of hash or encoding changes the
    /// meaning of every stored row and has to re-key them all.
    #[test]
    fn the_key_layout_is_fixed() {
        let instant = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).single().expect("a valid instant");
        let number: BigDecimal = "1.50".parse().expect("a decimal");
        assert_eq!(
            [
                SubAttributeKey::of(path(), KeyedValue::Number(&number)).get(),
                SubAttributeKey::of(path(), KeyedValue::Text("sensor-43", TextKind::String)).get(),
                SubAttributeKey::of(path(), KeyedValue::Instant(instant)).get(),
            ],
            [-8_030_412_358_756_010_887, -4_376_459_576_480_486_021, 588_906_301_028_363_066]
        );
    }
}
