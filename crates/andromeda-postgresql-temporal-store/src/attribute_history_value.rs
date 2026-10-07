//! The value one attribute-history row carries, and the column it occupies.

use crate::error::TemporalStoreError;
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::temporal::TemporalValue;
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_postgresql_codes::value_kind::ValueKind;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde_json::value::{RawValue, to_raw_value};

/// What one attribute-history row records.
///
/// The history table constrains a row so that its `value_kind` and the
/// typed column actually holding the value agree, and this enumeration is the
/// Rust half of that constraint: a row cannot be built that declares one shape
/// and populates another, so no reader has to guess which column to believe.
///
/// [`Tombstone`](AttributeHistoryValue::Tombstone) carries the deletion instant
/// and no value at all. The null representation `urn:ngsi-ld:null` is
/// synthesised at render time from the attribute family (clauses 4.5.7 and
/// 4.5.8) and is never written into a typed column, because a stored sentinel
/// is indistinguishable from a client that genuinely sent that string.
#[derive(Clone, Debug)]
pub enum AttributeHistoryValue {
    /// Ends the attribute instance at `deleted_at`.
    Tombstone {
        /// Instant the deletion took effect.
        deleted_at: DateTime<Utc>,
    },
    /// Arbitrary-precision number.
    Number(JsonNumber),
    /// A string that is not an `xsd:dateTime`.
    String(String),
    /// Boolean.
    Boolean(bool),
    /// A geometry, its positions stored in the broker's single geodetic
    /// reference system and its `bbox` and foreign members (IETF RFC 7946
    /// sections 5 and 6.1) as the JSON their own column stores them as.
    Geometry {
        /// Positions.
        form: GeometryForm,
        /// Stored JSON of the `bbox` and foreign members, when present.
        members: Option<Box<RawValue>>,
    },
    /// Structured value, as the JSON the structured column stores.
    Object(Box<RawValue>),
    /// A point in time, already truncated to the broker's microsecond resolution.
    DateTime(DateTime<Utc>),
    /// A URI, which is the shape a relationship object is stored in.
    Uri(Uri),
}

impl AttributeHistoryValue {
    /// The shape one neutral observation value is stored in.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::ScopeHasNoAttributeRow`] for an entity's
    /// Scope, which clause 4.5.4 gives no attribute stream: it is carried by a
    /// scope entry and stored in `scope_history`, so one that reached the
    /// attribute conversion would have to invent the stream the clause denies
    /// it; and [`TemporalStoreError::StoredJson`] when a structured value does
    /// not write as JSON.
    pub fn of_observation(value: &TemporalValue) -> Result<AttributeHistoryValue, TemporalStoreError> {
        Ok(match value {
            TemporalValue::Number(number) => AttributeHistoryValue::Number(number.clone()), // clone: the persisted row owns its exact decimal.
            TemporalValue::String(text) => AttributeHistoryValue::String(text.clone()),     // clone: the persisted row owns the string.
            TemporalValue::Boolean(flag) => AttributeHistoryValue::Boolean(*flag),
            TemporalValue::Geometry(geometry) => AttributeHistoryValue::Geometry {
                form: geometry.form().clone(), // clone: the persisted row owns its positions.
                members: geometry.members().map(to_raw_value).transpose().map_err(TemporalStoreError::StoredJson)?,
            },
            TemporalValue::Structured(structured) => AttributeHistoryValue::Object(structured.to_stored_json().map_err(TemporalStoreError::StoredJson)?),
            TemporalValue::DateTime(instant) => AttributeHistoryValue::DateTime(*instant),
            TemporalValue::Uri(uri) => AttributeHistoryValue::Uri(uri.clone()), // clone: the persisted row owns the URI.
            TemporalValue::Tombstone { deleted_at } => AttributeHistoryValue::Tombstone { deleted_at: *deleted_at },
            TemporalValue::Scopes(_) => return Err(TemporalStoreError::ScopeHasNoAttributeRow),
        })
    }

    /// Code the row records for this value's shape.
    #[must_use]
    pub const fn kind(&self) -> ValueKind {
        match self {
            AttributeHistoryValue::Tombstone { .. } => ValueKind::Tombstone,
            AttributeHistoryValue::Number(_) => ValueKind::Number,
            AttributeHistoryValue::String(_) => ValueKind::String,
            AttributeHistoryValue::Boolean(_) => ValueKind::Boolean,
            AttributeHistoryValue::Geometry { .. } => ValueKind::Geometry,
            AttributeHistoryValue::Object(_) => ValueKind::Object,
            AttributeHistoryValue::DateTime(_) => ValueKind::DateTime,
            AttributeHistoryValue::Uri(_) => ValueKind::Uri,
        }
    }

    /// Instant a tombstone records its deletion at.
    #[must_use]
    pub const fn deleted_at(&self) -> Option<DateTime<Utc>> {
        match self {
            AttributeHistoryValue::Tombstone { deleted_at } => Some(*deleted_at),
            AttributeHistoryValue::Number(_)
            | AttributeHistoryValue::String(_)
            | AttributeHistoryValue::Boolean(_)
            | AttributeHistoryValue::Geometry { .. }
            | AttributeHistoryValue::Object(_)
            | AttributeHistoryValue::DateTime(_)
            | AttributeHistoryValue::Uri(_) => None,
        }
    }

    /// `NUMERIC` literal for the numeric column.
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
            | AttributeHistoryValue::Geometry { .. }
            | AttributeHistoryValue::Object(_)
            | AttributeHistoryValue::DateTime(_)
            | AttributeHistoryValue::Uri(_) => None,
        }
    }

    /// Exact numeric value for a `NUMERIC` bind.
    #[must_use]
    pub const fn number(&self) -> Option<&BigDecimal> {
        match self {
            AttributeHistoryValue::Number(number) => Some(number.decimal()),
            AttributeHistoryValue::Tombstone { .. }
            | AttributeHistoryValue::String(_)
            | AttributeHistoryValue::Boolean(_)
            | AttributeHistoryValue::Geometry { .. }
            | AttributeHistoryValue::Object(_)
            | AttributeHistoryValue::DateTime(_)
            | AttributeHistoryValue::Uri(_) => None,
        }
    }

    /// String column content.
    #[must_use]
    pub fn string(&self) -> Option<&str> {
        match self {
            AttributeHistoryValue::String(text) => Some(text),
            AttributeHistoryValue::Tombstone { .. }
            | AttributeHistoryValue::Number(_)
            | AttributeHistoryValue::Boolean(_)
            | AttributeHistoryValue::Geometry { .. }
            | AttributeHistoryValue::Object(_)
            | AttributeHistoryValue::DateTime(_)
            | AttributeHistoryValue::Uri(_) => None,
        }
    }

    /// Boolean column content.
    #[must_use]
    pub const fn boolean(&self) -> Option<bool> {
        match self {
            AttributeHistoryValue::Boolean(flag) => Some(*flag),
            AttributeHistoryValue::Tombstone { .. }
            | AttributeHistoryValue::Number(_)
            | AttributeHistoryValue::String(_)
            | AttributeHistoryValue::Geometry { .. }
            | AttributeHistoryValue::Object(_)
            | AttributeHistoryValue::DateTime(_)
            | AttributeHistoryValue::Uri(_) => None,
        }
    }

    /// Geometry column content.
    #[must_use]
    pub const fn geometry(&self) -> Option<&GeometryForm> {
        match self {
            AttributeHistoryValue::Geometry { form, .. } => Some(form),
            AttributeHistoryValue::Tombstone { .. }
            | AttributeHistoryValue::Number(_)
            | AttributeHistoryValue::String(_)
            | AttributeHistoryValue::Boolean(_)
            | AttributeHistoryValue::Object(_)
            | AttributeHistoryValue::DateTime(_)
            | AttributeHistoryValue::Uri(_) => None,
        }
    }

    /// Geometry-members column content.
    #[must_use]
    pub fn geometry_members(&self) -> Option<&RawValue> {
        match self {
            AttributeHistoryValue::Geometry { members, .. } => members.as_deref(),
            AttributeHistoryValue::Tombstone { .. }
            | AttributeHistoryValue::Number(_)
            | AttributeHistoryValue::String(_)
            | AttributeHistoryValue::Boolean(_)
            | AttributeHistoryValue::Object(_)
            | AttributeHistoryValue::DateTime(_)
            | AttributeHistoryValue::Uri(_) => None,
        }
    }

    /// Structured column content.
    #[must_use]
    pub fn object(&self) -> Option<&RawValue> {
        match self {
            AttributeHistoryValue::Object(object) => Some(object),
            AttributeHistoryValue::Tombstone { .. }
            | AttributeHistoryValue::Number(_)
            | AttributeHistoryValue::String(_)
            | AttributeHistoryValue::Boolean(_)
            | AttributeHistoryValue::Geometry { .. }
            | AttributeHistoryValue::DateTime(_)
            | AttributeHistoryValue::Uri(_) => None,
        }
    }

    /// Timestamp column content.
    #[must_use]
    pub const fn date_time(&self) -> Option<DateTime<Utc>> {
        match self {
            AttributeHistoryValue::DateTime(instant) => Some(*instant),
            AttributeHistoryValue::Tombstone { .. }
            | AttributeHistoryValue::Number(_)
            | AttributeHistoryValue::String(_)
            | AttributeHistoryValue::Boolean(_)
            | AttributeHistoryValue::Geometry { .. }
            | AttributeHistoryValue::Object(_)
            | AttributeHistoryValue::Uri(_) => None,
        }
    }

    /// URI column content.
    #[must_use]
    pub const fn uri(&self) -> Option<&Uri> {
        match self {
            AttributeHistoryValue::Uri(uri) => Some(uri),
            AttributeHistoryValue::Tombstone { .. }
            | AttributeHistoryValue::Number(_)
            | AttributeHistoryValue::String(_)
            | AttributeHistoryValue::Boolean(_)
            | AttributeHistoryValue::Geometry { .. }
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
    use serde_json::value::RawValue;

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
        // One above 2^53, which the double format cannot name individually.
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
            AttributeHistoryValue::Object(RawValue::from_string(r#"{"a": 1}"#.to_owned()).expect("a JSON object")),
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
