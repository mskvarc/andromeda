//! The driver arguments of a rendered statement's values.
//!
//! sea-query renders a statement as text with numbered `$n` markers and hands
//! back its values in marker order; each value becomes the driver argument of
//! its marker here. An array value binds as one array argument whose elements
//! are optional, so an array holding NULL elements (a column of a parallel-array
//! set-write, where a row has no value) travels like any other value. A value
//! that cannot be encoded leaves the statement failing at execution with the
//! encode error, never a panic.
//!
//! The binding is the workspace's own because no published binder carries
//! both: one that unwraps every array element panics on the first NULL
//! element, and drops every encode error on the way.

use sea_query::{
    ArrayType,
    Value,
    ValueType,
    Values,
    prelude::{BigDecimal, DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc, Uuid},
};
use sqlx::{
    Arguments,
    Encode,
    Postgres,
    Type,
    error::BoxDynError,
    postgres::{PgArguments, PgHasArrayType},
};
use thiserror::Error;

/// Why a statement value has no driver argument.
#[derive(Debug, Error)]
pub(crate) enum UnbindableValue {
    /// An unsigned 64-bit value above the largest `bigint`.
    #[error("the unsigned value {0} exceeds the largest bigint")]
    UnsignedOutOfRange(u64),
    /// An array whose elements are not all of its declared element type.
    #[error("an array holds an element of another type than it declares")]
    MixedArray,
    /// An instant carrying a zone other than UTC, which the workspace never
    /// binds: every instant it stores is a UTC instant.
    #[error("an instant is bound in UTC only")]
    ZonedInstant,
    /// A database enum value, which the workspace binds as its text and casts
    /// where it is read instead.
    #[error("a database enum value has no binding; bind its text and cast it")]
    Enum,
}

/// The driver arguments of `values`, in marker order.
///
/// # Errors
///
/// Returns the first value that cannot be encoded.
pub(crate) fn statement_arguments(values: Values) -> Result<PgArguments, BoxDynError> {
    let mut arguments = PgArguments::default();
    for value in values.0 {
        bind_value(&mut arguments, value)?;
    }
    Ok(arguments)
}

/// Add one value as the next argument.
fn bind_value(arguments: &mut PgArguments, value: Value) -> Result<(), BoxDynError> {
    match value {
        Value::Bool(value) => arguments.add(value),
        // `i8` is PostgreSQL's single-byte `"char"`, not a number, so a tiny
        // integer binds as `smallint`.
        Value::TinyInt(value) => arguments.add(value.map(i16::from)),
        Value::SmallInt(value) => arguments.add(value),
        Value::Int(value) => arguments.add(value),
        Value::BigInt(value) => arguments.add(value),
        Value::TinyUnsigned(value) => arguments.add(value.map(i16::from)),
        Value::SmallUnsigned(value) => arguments.add(value.map(i32::from)),
        Value::Unsigned(value) => arguments.add(value.map(i64::from)),
        Value::BigUnsigned(value) => arguments.add(value.map(signed).transpose()?),
        Value::Float(value) => arguments.add(value),
        Value::Double(value) => arguments.add(value),
        Value::String(value) => arguments.add(value),
        Value::Enum(_) => Err(UnbindableValue::Enum.into()),
        Value::Char(value) => arguments.add(value.map(String::from)),
        Value::Bytes(value) => arguments.add(value),
        Value::ChronoDate(value) => arguments.add(value),
        Value::ChronoTime(value) => arguments.add(value),
        Value::ChronoDateTime(value) => arguments.add(value),
        Value::ChronoDateTimeUtc(value) => arguments.add(value),
        Value::ChronoDateTimeLocal(_) | Value::ChronoDateTimeWithTimeZone(_) => Err(UnbindableValue::ZonedInstant.into()),
        Value::Uuid(value) => arguments.add(value),
        Value::BigDecimal(value) => arguments.add(value.map(|value| *value)),
        Value::Array(element, elements) => bind_array(arguments, &element, elements.map(|elements| *elements)),
    }
}

/// Add one array value as the next argument, its elements optional.
fn bind_array(arguments: &mut PgArguments, element: &ArrayType, elements: Option<Vec<Value>>) -> Result<(), BoxDynError> {
    match element {
        ArrayType::Bool => add_array::<bool, _>(arguments, elements, Ok),
        ArrayType::TinyInt => add_array::<i8, _>(arguments, elements, |value| Ok(i16::from(value))),
        ArrayType::SmallInt => add_array::<i16, _>(arguments, elements, Ok),
        ArrayType::Int => add_array::<i32, _>(arguments, elements, Ok),
        ArrayType::BigInt => add_array::<i64, _>(arguments, elements, Ok),
        ArrayType::TinyUnsigned => add_array::<u8, _>(arguments, elements, |value| Ok(i16::from(value))),
        ArrayType::SmallUnsigned => add_array::<u16, _>(arguments, elements, |value| Ok(i32::from(value))),
        ArrayType::Unsigned => add_array::<u32, _>(arguments, elements, |value| Ok(i64::from(value))),
        ArrayType::BigUnsigned => add_array::<u64, _>(arguments, elements, signed),
        ArrayType::Float => add_array::<f32, _>(arguments, elements, Ok),
        ArrayType::Double => add_array::<f64, _>(arguments, elements, Ok),
        ArrayType::String => add_array::<String, _>(arguments, elements, Ok),
        ArrayType::Enum(_) => Err(UnbindableValue::Enum.into()),
        ArrayType::Char => add_array::<char, _>(arguments, elements, |value| Ok(String::from(value))),
        ArrayType::Bytes => add_array::<Vec<u8>, _>(arguments, elements, Ok),
        ArrayType::ChronoDate => add_array::<NaiveDate, _>(arguments, elements, Ok),
        ArrayType::ChronoTime => add_array::<NaiveTime, _>(arguments, elements, Ok),
        ArrayType::ChronoDateTime => add_array::<NaiveDateTime, _>(arguments, elements, Ok),
        ArrayType::ChronoDateTimeUtc => add_array::<DateTime<Utc>, _>(arguments, elements, Ok),
        ArrayType::ChronoDateTimeLocal | ArrayType::ChronoDateTimeWithTimeZone => Err(UnbindableValue::ZonedInstant.into()),
        ArrayType::Uuid => add_array::<Uuid, _>(arguments, elements, Ok),
        ArrayType::BigDecimal => add_array::<BigDecimal, _>(arguments, elements, Ok),
    }
}

/// Add `elements`, each read as an optional `S` and bound as `T`, as one array
/// argument.
fn add_array<S, T>(arguments: &mut PgArguments, elements: Option<Vec<Value>>, bound: impl Fn(S) -> Result<T, UnbindableValue>) -> Result<(), BoxDynError>
where
    Option<S>: ValueType,
    T: for<'q> Encode<'q, Postgres> + Type<Postgres> + PgHasArrayType + Send,
{
    let element = |value: Value| {
        <Option<S> as ValueType>::try_from(value)
            .map_err(|_mismatched| UnbindableValue::MixedArray)?
            .map(&bound)
            .transpose()
    };
    let elements = elements
        .map(|elements| elements.into_iter().map(element).collect::<Result<Vec<Option<T>>, UnbindableValue>>())
        .transpose()?;
    arguments.add(elements)
}

/// An unsigned 64-bit value as a `bigint`, when it fits.
fn signed(value: u64) -> Result<i64, UnbindableValue> {
    <i64 as TryFrom<u64>>::try_from(value).map_err(|_overflow| UnbindableValue::UnsignedOutOfRange(value))
}

#[cfg(test)]
mod tests {
    use crate::statement_arguments::statement_arguments;
    use sea_query::{ArrayType, Value, Values};
    use sqlx::Arguments;

    #[test]
    fn every_value_becomes_one_argument_in_marker_order() {
        let arguments = statement_arguments(Values(vec![
            Value::Int(Some(1)),
            Value::String(None),
            Value::BigUnsigned(Some(7)),
            Value::Bool(Some(true)),
        ]))
        .expect("bindable values");
        pretty_assertions::assert_eq!(arguments.len(), 4);
    }

    #[test]
    fn an_array_with_null_elements_binds_as_one_argument() {
        let elements = vec![Value::String(Some("a".to_owned())), Value::String(None)];
        let arguments = statement_arguments(Values(vec![Value::Array(ArrayType::String, Some(Box::new(elements)))])).expect("a bindable array");
        pretty_assertions::assert_eq!(arguments.len(), 1);
    }

    #[test]
    fn an_array_holding_an_element_of_another_type_is_refused() {
        let elements = vec![Value::Int(Some(1)), Value::String(Some("a".to_owned()))];
        let refusal = statement_arguments(Values(vec![Value::Array(ArrayType::Int, Some(Box::new(elements)))])).expect_err("a mixed array");
        pretty_assertions::assert_eq!(refusal.to_string(), "an array holds an element of another type than it declares");
    }

    #[test]
    fn an_unsigned_value_above_the_largest_bigint_is_refused() {
        let refusal = statement_arguments(Values(vec![Value::BigUnsigned(Some(u64::MAX))])).expect_err("an unsigned overflow");
        pretty_assertions::assert_eq!(refusal.to_string(), "the unsigned value 18446744073709551615 exceeds the largest bigint");
    }
}
