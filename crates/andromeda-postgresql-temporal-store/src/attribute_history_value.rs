//! The value one attribute-history row carries.

use crate::{error::TemporalStoreError, temporal_store_invariant::TemporalStoreInvariant};
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, reference::NgsiLdReference};
use andromeda_ngsi_ld_model::temporal::{TemporalValue, text_annotation::TextAnnotation};
use andromeda_postgresql_attribute_instance::{
    name_keys::NameKeys,
    stored_json::StoredJson,
    stored_value::{LiteralAnnotation, StoredValue},
};
use andromeda_postgresql_stored_values::entity_type_name_key::EntityTypeNameKey;
use chrono::{DateTime, Utc};
use serde_json::value::to_raw_value;

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
                number: number.clone(),
                datatype: key(datatype)?,
            },
            TemporalValue::String { text, annotation } => StoredValue::String {
                text: text.clone(),
                annotation: match annotation {
                    TextAnnotation::Plain => LiteralAnnotation::Plain,
                    TextAnnotation::Datatype(datatype) => LiteralAnnotation::Datatype(datatype_key(datatype, keys)?),
                    TextAnnotation::Language(language) => LiteralAnnotation::Language(language.clone()),
                },
            },
            TemporalValue::Boolean { flag, datatype } => StoredValue::Boolean {
                flag: *flag,
                datatype: key(datatype)?,
            },
            TemporalValue::DeclaredNumber { number, datatype } => StoredValue::DeclaredNumber {
                number: number.clone(),
                datatype: key(datatype)?,
            },
            TemporalValue::DeclaredBoolean { flag, datatype } => StoredValue::DeclaredBoolean {
                flag: *flag,
                datatype: key(datatype)?,
            },
            TemporalValue::Geometry(geometry) => StoredValue::Geometry {
                form: geometry.form().clone(),
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
            TemporalValue::Uri(uri) => StoredValue::Uri(uri.clone()),
            TemporalValue::Tombstone { deleted_at } => return Ok(AttributeHistoryValue::Tombstone { deleted_at: *deleted_at }),
            TemporalValue::Scopes(_) => return Err(TemporalStoreError::ScopeHasNoAttributeRow),
        }))
    }
}

/// The `entity_type_names` key `keys` holds for `datatype`.
fn datatype_key(datatype: &NgsiLdReference, keys: &NameKeys) -> Result<EntityTypeNameKey, TemporalStoreError> {
    keys.type_name(datatype)
        .ok_or_else(|| TemporalStoreInvariant::EntityTypeUnresolved(EntityTypeName::from(datatype)).into())
}

#[cfg(test)]
mod tests {
    use crate::attribute_history_value::AttributeHistoryValue;
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_ngsi_ld_model::temporal::TemporalValue;
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_postgresql_attribute_instance::name_keys::NameKeys;
    use andromeda_postgresql_stored_values::{entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};

    /// Every datatype was resolved before the value is encoded, so one that
    /// was not is an invariant failure rather than a silently plain value.
    #[test]
    fn an_unresolved_datatype_is_refused() {
        let keys = NameKeys::new(
            ResolvedNames::default(),
            ResolvedNames::from_pairs([(
                "http://www.w3.org/2001/XMLSchema#integer".parse::<EntityTypeName>().expect("a type name"),
                EntityTypeNameKey::from(12),
            )]),
        );
        let value = TemporalValue::Number {
            number: JsonNumber::from(7),
            datatype: Some("http://www.w3.org/2001/XMLSchema#double".parse().expect("a datatype")),
        };
        assert!(AttributeHistoryValue::of_observation(&value, &keys).is_err());
    }
}
