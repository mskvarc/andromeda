//! The JSON number a stored `NUMERIC` column names.
//!
//! `attribute_history.value_number` is `NUMERIC` and every arithmetic aggregate
//! PostgreSQL computes over it is `NUMERIC` as well, so both are read as an
//! arbitrary-precision decimal and converted directly into the same JSON numeric
//! model the value arrived in (`docs/exact-numeric-representation.md`). Nothing
//! here widens a decimal to a double on the way past: an integer beyond 2^53
//! comes back as that integer, and a literal the JSON numeric model cannot
//! carry fails the decode instead of becoming a rounded value or a null.

use crate::error::TemporalStoreError;
use andromeda_ngsi_ld_number::JsonNumber;
use bigdecimal::BigDecimal;

/// The JSON number a stored decimal names, reported against the column it came from.
///
/// # Errors
///
/// Returns [`TemporalStoreError::Decode`] when the database contains a value
/// outside the broker's documented numeric range.
pub(crate) fn json_number_of(decimal: &BigDecimal, column: &str) -> Result<JsonNumber, TemporalStoreError> {
    JsonNumber::from_decimal(decimal).map_err(|error| TemporalStoreError::Decode(format!("{column}: {error}")))
}

#[cfg(test)]
mod tests {
    use crate::stored_decimal::json_number_of;
    use andromeda_ngsi_ld_number::JsonNumber;
    use bigdecimal::BigDecimal;
    use std::str::FromStr;

    fn decimal(literal: &str) -> BigDecimal {
        BigDecimal::from_str(literal).expect("a decimal literal")
    }

    /// A `SUM` over integers is an integer, and one beyond 2^53 has no exact
    /// double: reading it through the JSON numeric model keeps it whole.
    #[test]
    fn a_sum_beyond_the_exact_double_range_keeps_its_integer_shape() {
        let number = json_number_of(&decimal("9007199254740993"), "agg_sum").expect("an integer aggregate");
        pretty_assertions::assert_eq!(number, JsonNumber::from(9_007_199_254_740_993_i64));
        pretty_assertions::assert_eq!(serde_json::to_string(&number).expect("a JSON number"), "9007199254740993");
    }

    /// An integer above the signed range is still an integer rather than the
    /// double nearest to it.
    #[test]
    fn an_integer_above_the_signed_range_keeps_its_unsigned_shape() {
        let number = json_number_of(&decimal("18446744073709551615"), "agg_sum").expect("an integer aggregate");
        pretty_assertions::assert_eq!(number, JsonNumber::from(u64::MAX));
    }

    /// An average is generally fractional, and the fractional part survives the
    /// decode rather than being truncated to the integer around it.
    #[test]
    fn a_fractional_average_keeps_its_fraction() {
        let number = json_number_of(&decimal("2.5"), "agg_avg").expect("a fractional aggregate");
        pretty_assertions::assert_eq!(number, "2.5".parse::<JsonNumber>().expect("an exact decimal"));
    }

    #[test]
    fn a_negative_aggregate_keeps_its_sign() {
        let number = json_number_of(&decimal("-273.15"), "agg_min").expect("a negative aggregate");
        pretty_assertions::assert_eq!(number, "-273.15".parse::<JsonNumber>().expect("an exact decimal"));
    }

    /// A zero-valued aggregate — the standard deviation of a single
    /// observation, for one — is a number like any other, whatever scale the
    /// column reported it at.
    #[test]
    fn a_scaled_zero_is_a_number_like_any_other() {
        let number = json_number_of(&decimal("0.000"), "agg_stddev").expect("a zero aggregate");
        pretty_assertions::assert_eq!(number, JsonNumber::from(0));
        pretty_assertions::assert_eq!(serde_json::to_string(&number).expect("a JSON number"), "0");
    }

    /// The failure names the column, because one aggregation row carries
    /// several decimal aggregates and the literal alone does not say which one
    /// was unreadable.
    #[test]
    fn a_decode_failure_names_the_column_it_came_from() {
        let error = json_number_of(&decimal("1e131072"), "agg_sumsq").expect_err("an unrepresentable magnitude is refused");
        assert!(error.to_string().contains("agg_sumsq"), "got {error}");
    }
}
