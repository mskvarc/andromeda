//! The value of the number column: a PostgreSQL `numeric`, finite or not.
//!
//! `numeric` holds `NaN`, `Infinity` and `-Infinity` beside its finite
//! decimals. The driver's decimal codec has none of the three, so this type
//! reads and writes them itself and hands every finite value to that codec,
//! which keeps the base-10000 digit conversion.
//!
//! A binary `numeric` opens with an 8-byte header of four big-endian 16-bit
//! words: the digit count, the weight, the sign word and the display scale.
//! The sign word tells a finite value (`0x0000` positive, `0x4000` negative)
//! from a special one (`0xC000` not-a-number, `0xD000` positive infinity,
//! `0xF000` negative infinity), which carries no digits.

use andromeda_ngsi_ld_number::{
    JsonNumber,
    JsonNumberError,
    declared_number::{DeclaredNumber, NonFinite},
};
use bigdecimal::BigDecimal;
use sqlx::{
    Decode,
    Encode,
    Postgres,
    Type,
    encode::IsNull,
    error::BoxDynError,
    postgres::{PgArgumentBuffer, PgHasArrayType, PgTypeInfo, PgValueFormat, PgValueRef},
};
use thiserror::Error;

/// The sign word of a positive finite value.
const SIGN_POSITIVE: u16 = 0x0000;
/// The sign word of a negative finite value.
const SIGN_NEGATIVE: u16 = 0x4000;
/// The sign word of not-a-number.
const SIGN_NOT_A_NUMBER: u16 = 0xC000;
/// The sign word of positive infinity.
const SIGN_POSITIVE_INFINITY: u16 = 0xD000;
/// The sign word of negative infinity.
const SIGN_NEGATIVE_INFINITY: u16 = 0xF000;

/// The length of a binary `numeric` header, and the whole of a special value.
const HEADER_LENGTH: usize = 8;

/// A stored `numeric` the number column cannot hold.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum StoredNumberError {
    /// The value ends before its header does.
    #[error("a binary numeric of {length} bytes ends before its 8-byte header")]
    TruncatedHeader {
        /// The length of the value, in bytes.
        length: usize,
    },
    /// The header's sign word is none PostgreSQL defines.
    #[error("a binary numeric carries the sign word {word:#06X}, which names no numeric value")]
    UnknownSign {
        /// The sign word.
        word: u16,
    },
}

/// One value of the number column, in current state, in the temporal history,
/// in a sub-attribute element and in an aggregation row alike.
///
/// A finite value is the decimal the driver reads, unchecked: whether the
/// number model carries it is decided where it becomes a [`DeclaredNumber`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredNumber {
    /// A finite decimal.
    Finite(BigDecimal),
    /// A value beyond the decimals.
    NonFinite(NonFinite),
}

// Hand-written: the decimal is taken out of the number, which no derive does.
impl From<JsonNumber> for StoredNumber {
    fn from(number: JsonNumber) -> StoredNumber {
        StoredNumber::Finite(number.into_decimal())
    }
}

// Hand-written: a finite number moves into another variant shape, which no
// derive expresses.
impl From<DeclaredNumber> for StoredNumber {
    fn from(number: DeclaredNumber) -> StoredNumber {
        match number {
            DeclaredNumber::Finite(number) => StoredNumber::from(number),
            DeclaredNumber::NonFinite(value) => StoredNumber::NonFinite(value),
        }
    }
}

impl TryFrom<StoredNumber> for DeclaredNumber {
    type Error = JsonNumberError;

    /// The declared number `number` is.
    ///
    /// # Errors
    ///
    /// Returns [`JsonNumberError::TooManyIntegralDigits`] or
    /// [`JsonNumberError::TooManyFractionalDigits`] for a finite decimal past
    /// the range the number model holds.
    fn try_from(number: StoredNumber) -> Result<DeclaredNumber, JsonNumberError> {
        match number {
            StoredNumber::Finite(decimal) => JsonNumber::from_decimal(&decimal).map(DeclaredNumber::Finite),
            StoredNumber::NonFinite(value) => Ok(DeclaredNumber::NonFinite(value)),
        }
    }
}

impl TryFrom<StoredNumber> for JsonNumber {
    type Error = JsonNumberError;

    /// The JSON number `number` is.
    ///
    /// # Errors
    ///
    /// Returns [`JsonNumberError::NotFinite`] for a value beyond the decimals,
    /// which no JSON number carries, and the range errors of
    /// [`DeclaredNumber`]'s conversion for a decimal past the range.
    fn try_from(number: StoredNumber) -> Result<JsonNumber, JsonNumberError> {
        JsonNumber::try_from(DeclaredNumber::try_from(number)?)
    }
}

/// What a binary header's sign word makes of the value.
#[derive(Debug, PartialEq, Eq)]
enum BinaryShape {
    /// A finite decimal, whose digits the driver's decoder reads.
    Finite,
    /// A special value, whole in its header.
    NonFinite(NonFinite),
}

/// The shape the header of the binary `numeric` `bytes` declares.
fn binary_shape(bytes: &[u8]) -> Result<BinaryShape, StoredNumberError> {
    let Some(&[_, _, _, _, high, low, _, _]) = bytes.get(..HEADER_LENGTH) else {
        return Err(StoredNumberError::TruncatedHeader { length: bytes.len() });
    };
    match u16::from_be_bytes([high, low]) {
        SIGN_POSITIVE | SIGN_NEGATIVE => Ok(BinaryShape::Finite),
        SIGN_NOT_A_NUMBER => Ok(BinaryShape::NonFinite(NonFinite::NotANumber)),
        SIGN_POSITIVE_INFINITY => Ok(BinaryShape::NonFinite(NonFinite::PositiveInfinity)),
        SIGN_NEGATIVE_INFINITY => Ok(BinaryShape::NonFinite(NonFinite::NegativeInfinity)),
        word => Err(StoredNumberError::UnknownSign { word }),
    }
}

/// The special value the text `numeric` `text` spells, or `None` for a
/// finite one.
fn text_shape(text: &str) -> Option<NonFinite> {
    match text {
        "NaN" => Some(NonFinite::NotANumber),
        "Infinity" => Some(NonFinite::PositiveInfinity),
        "-Infinity" => Some(NonFinite::NegativeInfinity),
        _finite => None,
    }
}

/// The binary form of the special `value`: its header with no digits, weight
/// 0 and display scale 0.
const fn non_finite_header(value: NonFinite) -> [u8; HEADER_LENGTH] {
    let [high, low] = match value {
        NonFinite::PositiveInfinity => SIGN_POSITIVE_INFINITY,
        NonFinite::NegativeInfinity => SIGN_NEGATIVE_INFINITY,
        NonFinite::NotANumber => SIGN_NOT_A_NUMBER,
    }
    .to_be_bytes();
    [0, 0, 0, 0, high, low, 0, 0]
}

/// The finite number the driver's decoder reads from `value`.
fn finite(value: PgValueRef<'_>) -> Result<StoredNumber, BoxDynError> {
    Ok(StoredNumber::Finite(<BigDecimal as Decode<'_, Postgres>>::decode(value)?))
}

// Hand-written: the column is `numeric`, the type the driver's decimal
// declares, which a derive cannot borrow.
impl Type<Postgres> for StoredNumber {
    fn type_info() -> PgTypeInfo {
        <BigDecimal as Type<Postgres>>::type_info()
    }
}

// Hand-written: see the `Type` implementation.
impl PgHasArrayType for StoredNumber {
    fn array_type_info() -> PgTypeInfo {
        <BigDecimal as PgHasArrayType>::array_type_info()
    }
}

// Hand-written: a finite value is encoded by the driver's decimal encoder and
// a special one as its header, which no derive expresses.
impl Encode<'_, Postgres> for StoredNumber {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        match self {
            StoredNumber::Finite(decimal) => <BigDecimal as Encode<'_, Postgres>>::encode_by_ref(decimal, buf),
            StoredNumber::NonFinite(value) => {
                buf.extend_from_slice(&non_finite_header(*value));
                Ok(IsNull::No)
            }
        }
    }

    fn size_hint(&self) -> usize {
        match self {
            StoredNumber::Finite(decimal) => <BigDecimal as Encode<'_, Postgres>>::size_hint(decimal),
            StoredNumber::NonFinite(_) => HEADER_LENGTH,
        }
    }
}

// Hand-written: a special value is read from its sign word or its text and a
// finite one handed to the driver's decimal decoder, which no derive
// expresses.
impl Decode<'_, Postgres> for StoredNumber {
    fn decode(value: PgValueRef<'_>) -> Result<StoredNumber, BoxDynError> {
        match value.format() {
            PgValueFormat::Binary => match binary_shape(value.as_bytes()?)? {
                BinaryShape::Finite => finite(value),
                BinaryShape::NonFinite(special) => Ok(StoredNumber::NonFinite(special)),
            },
            PgValueFormat::Text => match text_shape(value.as_str()?) {
                Some(special) => Ok(StoredNumber::NonFinite(special)),
                None => finite(value),
            },
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::stored_number::{BinaryShape, StoredNumber, StoredNumberError, binary_shape, non_finite_header, text_shape};
    use andromeda_ngsi_ld_number::{
        JsonNumber,
        JsonNumberError,
        declared_number::{DeclaredNumber, NonFinite},
    };
    use bigdecimal::BigDecimal;
    use sqlx::{Encode, Postgres, encode::IsNull, postgres::PgArgumentBuffer};
    use std::str::FromStr;

    /// The finite values the number column round-trips through the driver's
    /// decimal codec.
    fn finite_literals() -> Vec<String> {
        let mut literals = ["0", "-0", "1", "-1", "0.1", "1.5", "1E-7", "1E300"].map(String::from).to_vec();
        literals.push(format!("1{}", "7".repeat(999)));
        literals
    }

    /// The header of a binary `numeric` with no digits and the sign word
    /// `word`.
    fn header(word: u16) -> [u8; 8] {
        let [high, low] = word.to_be_bytes();
        [0, 0, 0, 0, high, low, 0, 0]
    }

    fn encoded(value: &impl for<'q> Encode<'q, Postgres>) -> Vec<u8> {
        let mut buffer = PgArgumentBuffer::default();
        let null = value.encode_by_ref(&mut buffer).expect("an encodable value");
        assert!(matches!(null, IsNull::No), "a number is never NULL");
        buffer.to_vec()
    }

    #[test]
    fn a_special_sign_word_reads_as_its_non_finite_value() {
        pretty_assertions::assert_eq!(binary_shape(&header(0xC000)), Ok(BinaryShape::NonFinite(NonFinite::NotANumber)));
        pretty_assertions::assert_eq!(binary_shape(&header(0xD000)), Ok(BinaryShape::NonFinite(NonFinite::PositiveInfinity)));
        pretty_assertions::assert_eq!(binary_shape(&header(0xF000)), Ok(BinaryShape::NonFinite(NonFinite::NegativeInfinity)));
    }

    #[test]
    fn a_finite_sign_word_is_left_to_the_decimal_decoder() {
        pretty_assertions::assert_eq!(binary_shape(&header(0x0000)), Ok(BinaryShape::Finite));
        let negative_one = [0, 1, 0, 0, 0x40, 0, 0, 0, 0, 1];
        pretty_assertions::assert_eq!(binary_shape(&negative_one), Ok(BinaryShape::Finite));
    }

    #[test]
    fn an_undefined_sign_word_is_refused() {
        for word in [0x0001, 0x2000, 0x8000, 0xC001, 0xE000, 0xFFFF] {
            pretty_assertions::assert_eq!(binary_shape(&header(word)), Err(StoredNumberError::UnknownSign { word }), "{word:#06X}");
        }
    }

    #[test]
    fn a_value_shorter_than_its_header_is_refused() {
        for length in [0, 5, 7] {
            let bytes = vec![0; length];
            pretty_assertions::assert_eq!(binary_shape(&bytes), Err(StoredNumberError::TruncatedHeader { length }));
        }
    }

    #[test]
    fn a_special_text_reads_as_its_non_finite_value_and_any_other_text_is_finite() {
        pretty_assertions::assert_eq!(text_shape("NaN"), Some(NonFinite::NotANumber));
        pretty_assertions::assert_eq!(text_shape("Infinity"), Some(NonFinite::PositiveInfinity));
        pretty_assertions::assert_eq!(text_shape("-Infinity"), Some(NonFinite::NegativeInfinity));
        for text in ["0", "-1.5", "1E+300", "nan", "inf", "-INF"] {
            pretty_assertions::assert_eq!(text_shape(text), None, "{text}");
        }
    }

    /// PostgreSQL's `numeric_send` writes a special value as a header with no
    /// digits, weight 0, its sign word and display scale 0.
    #[test]
    fn a_non_finite_value_is_written_as_its_bare_header() {
        let cases = [
            (NonFinite::NotANumber, [0, 0, 0, 0, 0xC0, 0, 0, 0]),
            (NonFinite::PositiveInfinity, [0, 0, 0, 0, 0xD0, 0, 0, 0]),
            (NonFinite::NegativeInfinity, [0, 0, 0, 0, 0xF0, 0, 0, 0]),
        ];
        for (value, bytes) in cases {
            pretty_assertions::assert_eq!(non_finite_header(value), bytes);
            pretty_assertions::assert_eq!(encoded(&StoredNumber::NonFinite(value)), bytes.to_vec());
            pretty_assertions::assert_eq!(binary_shape(&bytes), Ok(BinaryShape::NonFinite(value)));
        }
    }

    #[test]
    fn a_finite_value_is_written_as_the_decimal_encoder_writes_it() {
        for literal in finite_literals() {
            let decimal = BigDecimal::from_str(&literal).expect("a decimal");
            pretty_assertions::assert_eq!(encoded(&StoredNumber::Finite(decimal.clone())), encoded(&decimal), "{literal}");
            let number = JsonNumber::from_str(&literal).expect("a JSON number");
            pretty_assertions::assert_eq!(encoded(&StoredNumber::from(number.clone())), encoded(number.decimal()), "{literal}");
        }
    }

    #[test]
    fn a_stored_number_and_a_declared_number_name_the_same_value() {
        for literal in finite_literals() {
            let number = JsonNumber::from_str(&literal).expect("a JSON number");
            let declared = DeclaredNumber::Finite(number);
            pretty_assertions::assert_eq!(
                DeclaredNumber::try_from(StoredNumber::from(declared.clone())).expect("in range"),
                declared,
                "{literal}"
            );
        }
        for value in [NonFinite::PositiveInfinity, NonFinite::NegativeInfinity, NonFinite::NotANumber] {
            pretty_assertions::assert_eq!(StoredNumber::from(DeclaredNumber::NonFinite(value)), StoredNumber::NonFinite(value));
            pretty_assertions::assert_eq!(
                DeclaredNumber::try_from(StoredNumber::NonFinite(value)).expect("a declared value"),
                DeclaredNumber::NonFinite(value)
            );
        }
    }

    #[test]
    fn a_stored_decimal_past_the_number_range_is_no_declared_number() {
        let decimal = BigDecimal::from_str("1e131072").expect("a decimal");
        assert!(matches!(
            DeclaredNumber::try_from(StoredNumber::Finite(decimal)),
            Err(JsonNumberError::TooManyIntegralDigits)
        ));
    }
}
