//! The value one attribute-history row carries, and the column it occupies.

use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_postgresql_codes::value_kind::ValueKind;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// What one attribute-history row records.
///
/// The rebuilt history table constrains a row so that its `value_kind` and the
/// typed column actually holding the value agree, and this enumeration is the
/// Rust half of that constraint: a row cannot be built that declares one shape
/// and populates another, so no reader has to guess which column to believe.
///
/// [`Tombstone`](AttributeHistoryValue::Tombstone) carries the deletion instant
/// and no value at all. The null representation `urn:ngsi-ld:null` is
/// synthesised at render time from the attribute family (clauses 4.5.7 and
/// 4.5.8) and is never written into a typed column, because a stored sentinel
/// is indistinguishable from a client that genuinely sent that string.
#[derive(Clone, Debug, PartialEq)]
pub enum AttributeHistoryValue {
    /// The row ends the attribute instance at the given instant.
    Tombstone {
        /// The instant the deletion took effect.
        deleted_at: DateTime<Utc>,
    },
    /// An arbitrary-precision number.
    Number(JsonNumber),
    /// A string that is not an `xsd:dateTime`.
    String(String),
    /// A boolean.
    Boolean(bool),
    /// A geometry, stored in the broker's single geodetic reference system.
    Geometry(Geometry),
    /// A structured value: object, array, language map, JSON literal or list.
    Object(Value),
    /// A point in time, already truncated to the broker's microsecond resolution.
    DateTime(DateTime<Utc>),
    /// A URI, which is the shape a relationship object is stored in.
    Uri(Uri),
}

impl AttributeHistoryValue {
    /// The code the row records for this value's shape.
    #[must_use]
    pub const fn kind(&self) -> ValueKind {
        match self {
            AttributeHistoryValue::Tombstone { .. } => ValueKind::Tombstone,
            AttributeHistoryValue::Number(_) => ValueKind::Number,
            AttributeHistoryValue::String(_) => ValueKind::String,
            AttributeHistoryValue::Boolean(_) => ValueKind::Boolean,
            AttributeHistoryValue::Geometry(_) => ValueKind::Geometry,
            AttributeHistoryValue::Object(_) => ValueKind::Object,
            AttributeHistoryValue::DateTime(_) => ValueKind::DateTime,
            AttributeHistoryValue::Uri(_) => ValueKind::Uri,
        }
    }

    /// The instant this row records a deletion at, when it records one.
    #[must_use]
    pub const fn deleted_at(&self) -> Option<DateTime<Utc>> {
        match self {
            AttributeHistoryValue::Tombstone { deleted_at } => Some(*deleted_at),
            AttributeHistoryValue::Number(_)
            | AttributeHistoryValue::String(_)
            | AttributeHistoryValue::Boolean(_)
            | AttributeHistoryValue::Geometry(_)
            | AttributeHistoryValue::Object(_)
            | AttributeHistoryValue::DateTime(_)
            | AttributeHistoryValue::Uri(_) => None,
        }
    }

    /// The `NUMERIC` literal for the numeric column, when this row populates it.
    ///
    /// The column is arbitrary-precision, so the value is handed over as its
    /// decimal text rather than as a double: a JSON integer beyond the range the
    /// double format names individually reaches storage unchanged instead of
    /// being refused or rounded at the boundary. Every [`JsonNumber`] renders a
    /// finite literal without an exponent, which is exactly what `NUMERIC`
    /// parses.
    #[must_use]
    pub fn numeric_literal(&self) -> Option<String> {
        match self {
            AttributeHistoryValue::Number(number) => Some(number.to_string()),
            AttributeHistoryValue::Tombstone { .. }
            | AttributeHistoryValue::String(_)
            | AttributeHistoryValue::Boolean(_)
            | AttributeHistoryValue::Geometry(_)
            | AttributeHistoryValue::Object(_)
            | AttributeHistoryValue::DateTime(_)
            | AttributeHistoryValue::Uri(_) => None,
        }
    }

    /// Borrows the exact numeric value for an ordinary `SQLx` `NUMERIC` bind.
    #[must_use]
    pub const fn number(&self) -> Option<&BigDecimal> {
        match self {
            AttributeHistoryValue::Number(number) => Some(number.decimal()),
            AttributeHistoryValue::Tombstone { .. }
            | AttributeHistoryValue::String(_)
            | AttributeHistoryValue::Boolean(_)
            | AttributeHistoryValue::Geometry(_)
            | AttributeHistoryValue::Object(_)
            | AttributeHistoryValue::DateTime(_)
            | AttributeHistoryValue::Uri(_) => None,
        }
    }

    /// The string column's content, when this row populates it.
    #[must_use]
    pub fn string(&self) -> Option<&str> {
        match self {
            AttributeHistoryValue::String(text) => Some(text),
            AttributeHistoryValue::Tombstone { .. }
            | AttributeHistoryValue::Number(_)
            | AttributeHistoryValue::Boolean(_)
            | AttributeHistoryValue::Geometry(_)
            | AttributeHistoryValue::Object(_)
            | AttributeHistoryValue::DateTime(_)
            | AttributeHistoryValue::Uri(_) => None,
        }
    }

    /// The boolean column's content, when this row populates it.
    #[must_use]
    pub const fn boolean(&self) -> Option<bool> {
        match self {
            AttributeHistoryValue::Boolean(flag) => Some(*flag),
            AttributeHistoryValue::Tombstone { .. }
            | AttributeHistoryValue::Number(_)
            | AttributeHistoryValue::String(_)
            | AttributeHistoryValue::Geometry(_)
            | AttributeHistoryValue::Object(_)
            | AttributeHistoryValue::DateTime(_)
            | AttributeHistoryValue::Uri(_) => None,
        }
    }

    /// The geometry column's content, when this row populates it.
    #[must_use]
    pub const fn geometry(&self) -> Option<&Geometry> {
        match self {
            AttributeHistoryValue::Geometry(geometry) => Some(geometry),
            AttributeHistoryValue::Tombstone { .. }
            | AttributeHistoryValue::Number(_)
            | AttributeHistoryValue::String(_)
            | AttributeHistoryValue::Boolean(_)
            | AttributeHistoryValue::Object(_)
            | AttributeHistoryValue::DateTime(_)
            | AttributeHistoryValue::Uri(_) => None,
        }
    }

    /// The structured column's content, when this row populates it.
    #[must_use]
    pub const fn object(&self) -> Option<&Value> {
        match self {
            AttributeHistoryValue::Object(object) => Some(object),
            AttributeHistoryValue::Tombstone { .. }
            | AttributeHistoryValue::Number(_)
            | AttributeHistoryValue::String(_)
            | AttributeHistoryValue::Boolean(_)
            | AttributeHistoryValue::Geometry(_)
            | AttributeHistoryValue::DateTime(_)
            | AttributeHistoryValue::Uri(_) => None,
        }
    }

    /// The timestamp column's content, when this row populates it.
    #[must_use]
    pub const fn date_time(&self) -> Option<DateTime<Utc>> {
        match self {
            AttributeHistoryValue::DateTime(instant) => Some(*instant),
            AttributeHistoryValue::Tombstone { .. }
            | AttributeHistoryValue::Number(_)
            | AttributeHistoryValue::String(_)
            | AttributeHistoryValue::Boolean(_)
            | AttributeHistoryValue::Geometry(_)
            | AttributeHistoryValue::Object(_)
            | AttributeHistoryValue::Uri(_) => None,
        }
    }

    /// The URI column's content, when this row populates it.
    #[must_use]
    pub const fn uri(&self) -> Option<&Uri> {
        match self {
            AttributeHistoryValue::Uri(uri) => Some(uri),
            AttributeHistoryValue::Tombstone { .. }
            | AttributeHistoryValue::Number(_)
            | AttributeHistoryValue::String(_)
            | AttributeHistoryValue::Boolean(_)
            | AttributeHistoryValue::Geometry(_)
            | AttributeHistoryValue::Object(_)
            | AttributeHistoryValue::DateTime(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute_history_value::AttributeHistoryValue;
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_postgresql_codes::value_kind::ValueKind;
    use chrono::{TimeZone, Utc};

    #[test]
    fn a_tombstone_declares_the_reserved_kind_and_populates_no_typed_column() {
        let deleted_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        let value = AttributeHistoryValue::Tombstone { deleted_at };
        pretty_assertions::assert_eq!(value.kind(), ValueKind::Tombstone);
        pretty_assertions::assert_eq!(value.deleted_at(), Some(deleted_at));
        assert!(value.numeric_literal().is_none());
        assert!(value.string().is_none());
        assert!(value.boolean().is_none());
        assert!(value.geometry().is_none());
        assert!(value.object().is_none());
        assert!(value.date_time().is_none());
        assert!(value.uri().is_none());
    }

    #[test]
    fn an_integer_beyond_the_exact_double_range_reaches_the_numeric_column_unchanged() {
        // One above 2^53, which the double format cannot name individually and
        // the previous `DOUBLE PRECISION` column therefore refused outright.
        let value = AttributeHistoryValue::Number(JsonNumber::from(9_007_199_254_740_993_i64));
        pretty_assertions::assert_eq!(value.numeric_literal().as_deref(), Some("9007199254740993"));
    }

    #[test]
    fn a_fractional_number_renders_a_literal_numeric_parses() {
        let value = AttributeHistoryValue::Number(JsonNumber::try_from(-2.5e-4).expect("a finite number"));
        pretty_assertions::assert_eq!(value.numeric_literal().as_deref(), Some("-0.00025"));
    }

    #[test]
    fn a_non_tombstone_row_records_no_deletion_instant() {
        assert!(AttributeHistoryValue::Boolean(true).deleted_at().is_none());
    }

    #[test]
    fn every_value_shape_reports_exactly_one_populated_column() {
        let instant = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        let values = [
            AttributeHistoryValue::Number(JsonNumber::from(1)),
            AttributeHistoryValue::String("text".to_owned()),
            AttributeHistoryValue::Boolean(false),
            AttributeHistoryValue::Object(serde_json::json!({"a": 1})),
            AttributeHistoryValue::DateTime(instant),
            AttributeHistoryValue::Uri("urn:ngsi-ld:Entity:target".parse().expect("a valid URI")),
        ];
        for value in &values {
            let populated = usize::from(value.numeric_literal().is_some())
                + usize::from(value.string().is_some())
                + usize::from(value.boolean().is_some())
                + usize::from(value.geometry().is_some())
                + usize::from(value.object().is_some())
                + usize::from(value.date_time().is_some())
                + usize::from(value.uri().is_some());
            pretty_assertions::assert_eq!(populated, 1, "{value:?} populates {populated} columns");
        }
    }
}
