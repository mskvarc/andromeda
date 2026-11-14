//! The name a snapshot gives the type of a bound value.
//!
//! sea-query tags every [`Value`] with the type the driver binds it as (`Int`
//! as int4, `BigDecimal` as numeric, and so on). A snapshot records that tag
//! beside the value, so a change of the type a statement binds shows in the
//! diff. The match over [`Value`] lists every variant of the features this
//! crate enables and has no fallback: a feature that adds a variant stops the
//! build here instead of renaming a type in every snapshot.

use sea_query::Value;
use strum::Display;

/// The type tag of a bound value, spelled as the sea-query variant that holds
/// it.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum BoundValueType {
    /// A PostgreSQL array of any element type.
    Array,
    /// Numeric of arbitrary precision.
    BigDecimal,
    /// Signed 64 bits, int8.
    BigInt,
    /// Unsigned 64 bits, bound as numeric.
    BigUnsigned,
    /// Boolean.
    Bool,
    /// Binary string, bytea.
    Bytes,
    /// A single character.
    Char,
    /// Calendar date.
    ChronoDate,
    /// Timestamp without time zone.
    ChronoDateTime,
    /// Timestamp with time zone, held in the local zone.
    ChronoDateTimeLocal,
    /// Timestamp with time zone, held in UTC.
    ChronoDateTimeUtc,
    /// Timestamp with time zone, held with a fixed offset.
    ChronoDateTimeWithTimeZone,
    /// Time of day.
    ChronoTime,
    /// Double precision float, float8.
    Double,
    /// A PostgreSQL enumeration value.
    Enum,
    /// Single precision float, float4.
    Float,
    /// Signed 32 bits, int4.
    Int,
    /// Signed 16 bits, int2.
    SmallInt,
    /// Unsigned 16 bits.
    SmallUnsigned,
    /// Text.
    String,
    /// Signed 8 bits.
    TinyInt,
    /// Unsigned 8 bits.
    TinyUnsigned,
    /// Unsigned 32 bits.
    Unsigned,
    /// Universally unique identifier.
    Uuid,
}

impl From<&Value> for BoundValueType {
    fn from(value: &Value) -> Self {
        match value {
            Value::Array(..) => Self::Array,
            Value::BigDecimal(_) => Self::BigDecimal,
            Value::BigInt(_) => Self::BigInt,
            Value::BigUnsigned(_) => Self::BigUnsigned,
            Value::Bool(_) => Self::Bool,
            Value::Bytes(_) => Self::Bytes,
            Value::Char(_) => Self::Char,
            Value::ChronoDate(_) => Self::ChronoDate,
            Value::ChronoDateTime(_) => Self::ChronoDateTime,
            Value::ChronoDateTimeLocal(_) => Self::ChronoDateTimeLocal,
            Value::ChronoDateTimeUtc(_) => Self::ChronoDateTimeUtc,
            Value::ChronoDateTimeWithTimeZone(_) => Self::ChronoDateTimeWithTimeZone,
            Value::ChronoTime(_) => Self::ChronoTime,
            Value::Double(_) => Self::Double,
            Value::Enum(_) => Self::Enum,
            Value::Float(_) => Self::Float,
            Value::Int(_) => Self::Int,
            Value::SmallInt(_) => Self::SmallInt,
            Value::SmallUnsigned(_) => Self::SmallUnsigned,
            Value::String(_) => Self::String,
            Value::TinyInt(_) => Self::TinyInt,
            Value::TinyUnsigned(_) => Self::TinyUnsigned,
            Value::Unsigned(_) => Self::Unsigned,
            Value::Uuid(_) => Self::Uuid,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::bound_value_type::BoundValueType;
    use pretty_assertions::assert_eq;
    use sea_query::{ArrayType, Value};

    #[test]
    fn a_value_is_named_as_the_sea_query_variant_that_holds_it() {
        let named = [
            (Value::Int(Some(7)), "Int"),
            (Value::Int(None), "Int"),
            (Value::String(Some("(text".to_owned())), "String"),
            (Value::BigUnsigned(Some(1)), "BigUnsigned"),
            (Value::Double(Some(0.5)), "Double"),
            (Value::Bytes(Some(vec![1])), "Bytes"),
            (Value::Array(ArrayType::Int, None), "Array"),
        ];
        for (value, name) in named {
            assert_eq!(BoundValueType::from(&value).to_string(), name);
        }
    }
}
