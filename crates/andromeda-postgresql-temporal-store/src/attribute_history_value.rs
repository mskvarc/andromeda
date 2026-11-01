//! The value one attribute-history row carries, and the column it occupies.

use crate::{error::TemporalStoreError, temporal_store_invariant::TemporalStoreInvariant};
use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, reference::NgsiLdReference, uri::Uri};
use andromeda_ngsi_ld_model::temporal::{TemporalValue, text_annotation::TextAnnotation};
use andromeda_ngsi_ld_number::declared_number::DeclaredNumber;
use andromeda_postgresql_attribute_instance::{
    name_keys::NameKeys,
    stored_json::StoredJson,
    stored_number::StoredNumber,
    stored_value::{LiteralAnnotation, StoredValue},
};
use andromeda_postgresql_codes::value_kind::ValueKind;
use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
use chrono::{DateTime, Utc};
use langtag::LangTagBuf;
use serde_json::value::{RawValue, to_raw_value};
use std::borrow::Cow;

/// What one attribute-history row records: the end of an instance, or an
/// instance's value in the stored form current state holds it in.
///
/// The history table constrains a row so that its `value_kind` and the
/// typed column actually holding the value agree, and this enumeration is the
/// Rust half of that constraint: a row cannot be built that declares one shape
/// and populates another, so no reader has to guess which column to believe.
/// A recorded value is a [`StoredValue`], so an instance has one stored form,
/// with its datatype and language, whether it sits in a current row or a
/// history row.
///
/// [`Tombstone`](AttributeHistoryValue::Tombstone) carries the deletion instant
/// and no value at all. The null representation `urn:ngsi-ld:null` is
/// synthesised at render time from the attribute family (clauses 4.5.7 and
/// 4.5.8) and is never written into a typed column, because a stored sentinel
/// is indistinguishable from a client that genuinely sent that string.
#[derive(Debug)]
pub enum AttributeHistoryValue {
    /// Ends the attribute instance at `deleted_at`.
    Tombstone {
        /// Instant the deletion took effect.
        deleted_at: DateTime<Utc>,
    },
    /// An instance's value.
    Recorded(StoredValue),
}

impl AttributeHistoryValue {
    /// The shape one neutral observation value is stored in, every datatype
    /// it declares as its key in `keys`.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::ScopeHasNoAttributeRow`] for an entity's
    /// Scope, which clause 4.5.4 gives no attribute stream: it is carried by a
    /// scope entry and stored in `scope_history`, so one that reached the
    /// attribute conversion would have to invent the stream the clause denies
    /// it; [`TemporalStoreError::StoredJson`] when a structured value does not
    /// write as JSON; and an invariant failure when a datatype is missing
    /// from `keys`, which resolved every name the observation mentions.
    pub fn of_observation(value: &TemporalValue, keys: &NameKeys) -> Result<AttributeHistoryValue, TemporalStoreError> {
        let key = |datatype: &Option<NgsiLdReference>| datatype.as_ref().map(|datatype| datatype_key(datatype, keys)).transpose();
        Ok(AttributeHistoryValue::Recorded(match value {
            TemporalValue::Number { number, datatype } => StoredValue::Number {
                number: number.clone(), // clone: the persisted row owns its exact decimal.
                datatype: key(datatype)?,
            },
            TemporalValue::String { text, annotation } => StoredValue::String {
                text: text.clone(), // clone: the persisted row owns the string.
                annotation: match annotation {
                    TextAnnotation::Plain => LiteralAnnotation::Plain,
                    TextAnnotation::Datatype(datatype) => LiteralAnnotation::Datatype(datatype_key(datatype, keys)?),
                    // clone: the persisted row owns the tag.
                    TextAnnotation::Language(language) => LiteralAnnotation::Language(language.clone()),
                },
            },
            TemporalValue::Boolean { flag, datatype } => StoredValue::Boolean {
                flag: *flag,
                datatype: key(datatype)?,
            },
            TemporalValue::DeclaredNumber { number, datatype } => StoredValue::DeclaredNumber {
                number: number.clone(), // clone: the persisted row owns its number.
                datatype: key(datatype)?,
            },
            TemporalValue::DeclaredBoolean { flag, datatype } => StoredValue::DeclaredBoolean {
                flag: *flag,
                datatype: key(datatype)?,
            },
            TemporalValue::Geometry(geometry) => StoredValue::Geometry {
                form: geometry.form().clone(), // clone: the persisted row owns its positions.
                members: geometry
                    .members()
                    .map(to_raw_value)
                    .transpose()
                    .map_err(TemporalStoreError::StoredJson)?
                    .map(StoredJson::new),
            },
            TemporalValue::Structured(structured) => StoredValue::Object(StoredJson::new(structured.to_stored_json().map_err(TemporalStoreError::StoredJson)?)),
            TemporalValue::DateTime { instant, datatype } => StoredValue::DateTime {
                instant: *instant,
                datatype: key(datatype)?,
            },
            TemporalValue::Date { date, datatype } => StoredValue::Date {
                date: *date,
                datatype: key(datatype)?,
            },
            TemporalValue::Time { time, datatype } => StoredValue::Time {
                time: *time,
                datatype: key(datatype)?,
            },
            TemporalValue::Uri(uri) => StoredValue::Uri(uri.clone()), // clone: the persisted row owns the URI.
            TemporalValue::Tombstone { deleted_at } => return Ok(AttributeHistoryValue::Tombstone { deleted_at: *deleted_at }),
            TemporalValue::Scopes(_) => return Err(TemporalStoreError::ScopeHasNoAttributeRow),
        }))
    }

    /// Code the row records for this value's shape.
    #[must_use]
    pub const fn kind(&self) -> ValueKind {
        match self {
            AttributeHistoryValue::Tombstone { .. } => ValueKind::Tombstone,
            AttributeHistoryValue::Recorded(value) => value.kind(),
        }
    }

    /// Instant a tombstone records its deletion at.
    #[must_use]
    pub const fn deleted_at(&self) -> Option<DateTime<Utc>> {
        match self {
            AttributeHistoryValue::Tombstone { deleted_at } => Some(*deleted_at),
            AttributeHistoryValue::Recorded(_) => None,
        }
    }

    /// The recorded value, `None` for a tombstone.
    const fn recorded(&self) -> Option<&StoredValue> {
        match self {
            AttributeHistoryValue::Tombstone { .. } => None,
            AttributeHistoryValue::Recorded(value) => Some(value),
        }
    }

    /// `value_datatype_key` content: the datatype the literal declares.
    #[must_use]
    pub fn datatype_key(&self) -> Option<i32> {
        self.recorded().and_then(StoredValue::datatype_key).map(EntityTypeNameKey::get)
    }

    /// `value_language` content: the tag of a language-tagged string.
    #[must_use]
    pub fn language(&self) -> Option<&LangTagBuf> {
        self.recorded().and_then(StoredValue::language)
    }

    /// `NUMERIC` literal for the numeric column.
    ///
    /// The column is arbitrary-precision, so the value is handed over as its
    /// decimal text rather than as a double: a JSON integer beyond the range the
    /// double format names individually reaches storage unchanged instead of
    /// being refused or rounded at the boundary. Every finite number renders a
    /// literal without an exponent, which is exactly what `NUMERIC` parses; a
    /// number beyond the decimals has none.
    #[must_use]
    pub fn numeric_literal(&self) -> Option<String> {
        match self.recorded()? {
            StoredValue::Number { number, .. }
            | StoredValue::DeclaredNumber {
                number: DeclaredNumber::Finite(number),
                ..
            } => Some(number.to_string()),
            StoredValue::String { .. }
            | StoredValue::Boolean { .. }
            | StoredValue::DeclaredNumber { .. }
            | StoredValue::DeclaredBoolean { .. }
            | StoredValue::DateTime { .. }
            | StoredValue::Date { .. }
            | StoredValue::Time { .. }
            | StoredValue::Geometry { .. }
            | StoredValue::Object(_)
            | StoredValue::Uri(_) => None,
        }
    }

    /// `value_number` content: the exact number, for a `NUMERIC` bind.
    #[must_use]
    pub fn number(&self) -> Option<StoredNumber> {
        match self.recorded()? {
            // clone: the bind owns the decimal it encodes, and the value gives up none.
            StoredValue::Number { number, .. } => Some(StoredNumber::from(number.clone())),
            // clone: as above.
            StoredValue::DeclaredNumber { number, .. } => Some(StoredNumber::from(number.clone())),
            StoredValue::String { .. }
            | StoredValue::Boolean { .. }
            | StoredValue::DeclaredBoolean { .. }
            | StoredValue::DateTime { .. }
            | StoredValue::Date { .. }
            | StoredValue::Time { .. }
            | StoredValue::Geometry { .. }
            | StoredValue::Object(_)
            | StoredValue::Uri(_) => None,
        }
    }

    /// String column content: a String as it is, a `Date` in its one
    /// spelling, a `Time` in its fixed-width storage encoding.
    #[must_use]
    pub fn string(&self) -> Option<Cow<'_, str>> {
        match self.recorded()? {
            StoredValue::String { text, .. } => Some(Cow::Borrowed(text)),
            StoredValue::Date { date, .. } => Some(Cow::Owned(date.to_string())),
            StoredValue::Time { time, .. } => Some(Cow::Owned(time.to_string())),
            StoredValue::Number { .. }
            | StoredValue::DeclaredNumber { .. }
            | StoredValue::DeclaredBoolean { .. }
            | StoredValue::Boolean { .. }
            | StoredValue::DateTime { .. }
            | StoredValue::Geometry { .. }
            | StoredValue::Object(_)
            | StoredValue::Uri(_) => None,
        }
    }

    /// Boolean column content.
    #[must_use]
    pub fn boolean(&self) -> Option<bool> {
        match self.recorded()? {
            StoredValue::Boolean { flag, .. } | StoredValue::DeclaredBoolean { flag, .. } => Some(*flag),
            StoredValue::Number { .. }
            | StoredValue::DeclaredNumber { .. }
            | StoredValue::String { .. }
            | StoredValue::DateTime { .. }
            | StoredValue::Date { .. }
            | StoredValue::Time { .. }
            | StoredValue::Geometry { .. }
            | StoredValue::Object(_)
            | StoredValue::Uri(_) => None,
        }
    }

    /// Geometry column content.
    #[must_use]
    pub fn geometry(&self) -> Option<&GeometryForm> {
        match self.recorded()? {
            StoredValue::Geometry { form, .. } => Some(form),
            StoredValue::Number { .. }
            | StoredValue::DeclaredNumber { .. }
            | StoredValue::DeclaredBoolean { .. }
            | StoredValue::String { .. }
            | StoredValue::Boolean { .. }
            | StoredValue::DateTime { .. }
            | StoredValue::Date { .. }
            | StoredValue::Time { .. }
            | StoredValue::Object(_)
            | StoredValue::Uri(_) => None,
        }
    }

    /// Geometry-members column content.
    #[must_use]
    pub fn geometry_members(&self) -> Option<&RawValue> {
        match self.recorded()? {
            StoredValue::Geometry { members, .. } => members.as_ref().map(StoredJson::as_raw),
            StoredValue::Number { .. }
            | StoredValue::DeclaredNumber { .. }
            | StoredValue::DeclaredBoolean { .. }
            | StoredValue::String { .. }
            | StoredValue::Boolean { .. }
            | StoredValue::DateTime { .. }
            | StoredValue::Date { .. }
            | StoredValue::Time { .. }
            | StoredValue::Object(_)
            | StoredValue::Uri(_) => None,
        }
    }

    /// Structured column content.
    #[must_use]
    pub fn object(&self) -> Option<&RawValue> {
        match self.recorded()? {
            StoredValue::Object(object) => Some(object.as_raw()),
            StoredValue::Number { .. }
            | StoredValue::DeclaredNumber { .. }
            | StoredValue::DeclaredBoolean { .. }
            | StoredValue::String { .. }
            | StoredValue::Boolean { .. }
            | StoredValue::DateTime { .. }
            | StoredValue::Date { .. }
            | StoredValue::Time { .. }
            | StoredValue::Geometry { .. }
            | StoredValue::Uri(_) => None,
        }
    }

    /// Timestamp column content.
    #[must_use]
    pub fn date_time(&self) -> Option<DateTime<Utc>> {
        match self.recorded()? {
            StoredValue::DateTime { instant, .. } => Some(*instant),
            StoredValue::Number { .. }
            | StoredValue::DeclaredNumber { .. }
            | StoredValue::DeclaredBoolean { .. }
            | StoredValue::String { .. }
            | StoredValue::Boolean { .. }
            | StoredValue::Date { .. }
            | StoredValue::Time { .. }
            | StoredValue::Geometry { .. }
            | StoredValue::Object(_)
            | StoredValue::Uri(_) => None,
        }
    }

    /// URI column content.
    #[must_use]
    pub fn uri(&self) -> Option<&Uri> {
        match self.recorded()? {
            StoredValue::Uri(uri) => Some(uri),
            StoredValue::Number { .. }
            | StoredValue::DeclaredNumber { .. }
            | StoredValue::DeclaredBoolean { .. }
            | StoredValue::String { .. }
            | StoredValue::Boolean { .. }
            | StoredValue::DateTime { .. }
            | StoredValue::Date { .. }
            | StoredValue::Time { .. }
            | StoredValue::Geometry { .. }
            | StoredValue::Object(_) => None,
        }
    }
}

/// The `entity_type_names` key `keys` holds for `datatype`.
fn datatype_key(datatype: &NgsiLdReference, keys: &NameKeys) -> Result<EntityTypeNameKey, TemporalStoreError> {
    keys.type_name(datatype)
        .ok_or_else(|| TemporalStoreInvariant::EntityTypeUnresolved(EntityTypeName::from(datatype)).into())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::attribute_history_value::AttributeHistoryValue;
    use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, reference::NgsiLdReference};
    use andromeda_ngsi_ld_model::temporal::{TemporalValue, text_annotation::TextAnnotation};
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_postgresql_attribute_instance::{
        name_keys::NameKeys,
        stored_json::StoredJson,
        stored_value::{LiteralAnnotation, StoredValue},
    };
    use andromeda_postgresql_codes::value_kind::ValueKind;
    use andromeda_postgresql_query::{dictionary::resolved_names::ResolvedNames, entity_type_name_key::EntityTypeNameKey};
    use chrono::{TimeZone, Utc};
    use langtag::LangTagBuf;
    use serde_json::value::RawValue;

    const INTEGER: &str = "http://www.w3.org/2001/XMLSchema#integer";

    /// Keys holding `xsd:integer` at 12.
    fn keys() -> NameKeys {
        NameKeys::new(
            ResolvedNames::default(),
            ResolvedNames::from_pairs([(INTEGER.parse::<EntityTypeName>().expect("a type name"), EntityTypeNameKey::from(12))]),
        )
    }

    fn recorded(value: StoredValue) -> AttributeHistoryValue {
        AttributeHistoryValue::Recorded(value)
    }

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
        assert!(value.datatype_key().is_none());
        assert!(value.language().is_none());
    }

    #[test]
    fn an_integer_beyond_the_exact_double_range_reaches_the_numeric_column_unchanged() {
        // One above 2^53, which the double format cannot name individually.
        let value = recorded(StoredValue::Number {
            number: JsonNumber::from(9_007_199_254_740_993_i64),
            datatype: None,
        });
        pretty_assertions::assert_eq!(value.numeric_literal().as_deref(), Some("9007199254740993"));
    }

    #[test]
    fn a_fractional_number_renders_a_literal_numeric_parses() {
        let value = recorded(StoredValue::Number {
            number: JsonNumber::try_from(-2.5e-4).expect("a finite number"),
            datatype: None,
        });
        pretty_assertions::assert_eq!(value.numeric_literal().as_deref(), Some("-0.00025"));
    }

    #[test]
    fn a_non_tombstone_row_records_no_deletion_instant() {
        assert!(recorded(StoredValue::Boolean { flag: true, datatype: None }).deleted_at().is_none());
    }

    #[test]
    fn every_value_shape_reports_exactly_one_populated_column() {
        let instant = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        let values = [
            recorded(StoredValue::Number {
                number: JsonNumber::from(1),
                datatype: None,
            }),
            recorded(StoredValue::String {
                text: "text".to_owned(),
                annotation: LiteralAnnotation::Plain,
            }),
            recorded(StoredValue::Boolean { flag: false, datatype: None }),
            recorded(StoredValue::Object(StoredJson::new(
                RawValue::from_string(r#"{"a": 1}"#.to_owned()).expect("a JSON object"),
            ))),
            recorded(StoredValue::DateTime { instant, datatype: None }),
            recorded(StoredValue::Uri("urn:ngsi-ld:Entity:target".parse().expect("a valid URI"))),
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

    /// A typed literal keeps its datatype as its dictionary key, and a
    /// language-tagged string its tag, in the columns current state holds them
    /// in.
    #[test]
    fn a_literal_keeps_its_datatype_key_and_its_language() {
        let typed = AttributeHistoryValue::of_observation(
            &TemporalValue::String {
                text: "42".to_owned(),
                annotation: TextAnnotation::Datatype(INTEGER.parse::<NgsiLdReference>().expect("a datatype")),
            },
            &keys(),
        )
        .expect("the value encodes");
        pretty_assertions::assert_eq!((typed.datatype_key(), typed.language()), (Some(12), None));
        let tag = LangTagBuf::new("es".to_owned()).expect("a language tag");
        let tagged = AttributeHistoryValue::of_observation(
            &TemporalValue::String {
                text: "hola".to_owned(),
                annotation: TextAnnotation::Language(tag.clone()),
            },
            &keys(),
        )
        .expect("the value encodes");
        pretty_assertions::assert_eq!((tagged.datatype_key(), tagged.language()), (None, Some(&tag)));
    }

    /// Every datatype was resolved before the value is encoded, so one that
    /// was not is an invariant failure rather than a silently plain value.
    #[test]
    fn an_unresolved_datatype_is_refused() {
        let value = TemporalValue::Number {
            number: JsonNumber::from(7),
            datatype: Some("http://www.w3.org/2001/XMLSchema#double".parse().expect("a datatype")),
        };
        assert!(AttributeHistoryValue::of_observation(&value, &keys()).is_err());
    }
}
