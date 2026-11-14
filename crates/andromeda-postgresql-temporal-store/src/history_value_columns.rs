//! The value columns of one `attribute_history` row, as both history writers
//! bind them.
//!
//! An appended entry and an in-place replacement write the same columns, so a
//! value is split into them once: exactly one value field set, the one its
//! value kind is placed in, or none and `deleted_at` for the end of an
//! instance. A history row has no overflow column, so a String and a URI of any
//! length are held in their one field.

use crate::{
    attribute_history_value::AttributeHistoryValue,
    error::TemporalStoreError,
    scope_history_value::ScopeHistoryValue,
    temporal_store_invariant::TemporalStoreInvariant,
};
use andromeda_postgresql_attribute_instance::{
    placed_value::{FieldValue, PlacedValue},
    stored_json::StoredJson,
    stored_number::StoredNumber,
    stored_value::StoredValue,
    sub_attribute::EwkbGeometry,
};
use andromeda_postgresql_stored_values::{entity_type_name_key::EntityTypeNameKey, value_kind::ValueKind};
use chrono::{DateTime, Utc};
use serde_json::value::RawValue;
use sqlx::types::Json;

/// The value columns of one history row.
#[derive(Debug, Default)]
pub(crate) struct HistoryValueColumns {
    /// `value_kind`.
    pub(crate) value_kind: i16,
    /// `deleted_at`.
    pub(crate) deleted_at: Option<DateTime<Utc>>,
    /// `value_date_time`.
    pub(crate) value_date_time: Option<DateTime<Utc>>,
    /// `value_boolean`.
    pub(crate) value_boolean: Option<bool>,
    /// `value_number`.
    pub(crate) value_number: Option<StoredNumber>,
    /// `value_string`.
    pub(crate) value_string: Option<String>,
    /// `value_language`.
    pub(crate) value_language: Option<String>,
    /// `value_datatype_key`.
    pub(crate) value_datatype_key: Option<i32>,
    /// `value_geometry`.
    pub(crate) value_geometry: Option<EwkbGeometry>,
    /// `value_geometry_reach`, `None` where the reach stays within the
    /// tolerance of the planar bounding box.
    pub(crate) value_geometry_reach: Option<EwkbGeometry>,
    /// `value_geometry_members`.
    pub(crate) value_geometry_members: Option<Json<Box<RawValue>>>,
    /// `value_object`.
    pub(crate) value_object: Option<Json<Box<RawValue>>>,
    /// `value_uri`.
    pub(crate) value_uri: Option<String>,
}

impl HistoryValueColumns {
    /// The columns an attribute value is stored in.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Encoding`] when a geometry has no EWKB
    /// encoding, and an invariant failure for a data-integrity seal, which is
    /// only ever a sub-attribute (clause 4.5.2.2) and so no observation
    /// records.
    pub(crate) fn of_attribute(value: AttributeHistoryValue) -> Result<HistoryValueColumns, TemporalStoreError> {
        match value {
            AttributeHistoryValue::Tombstone { deleted_at } => Ok(HistoryValueColumns {
                value_kind: ValueKind::Tombstone.code(),
                deleted_at: Some(deleted_at),
                ..HistoryValueColumns::default()
            }),
            AttributeHistoryValue::Recorded(value) => HistoryValueColumns::of_recorded(value),
        }
    }

    /// The columns a Scope entry's value is stored in: its `scope_history`
    /// code and, for a deletion, its instant. The path travels in the Scope
    /// fields.
    pub(crate) fn of_scope(value: &ScopeHistoryValue) -> HistoryValueColumns {
        HistoryValueColumns {
            value_kind: value.kind().code(),
            deleted_at: value.deleted_at(),
            ..HistoryValueColumns::default()
        }
    }

    /// The columns an instance's value is stored in, in the field its kind is
    /// placed in.
    fn of_recorded(value: StoredValue) -> Result<HistoryValueColumns, TemporalStoreError> {
        let mut columns = HistoryValueColumns {
            value_kind: value.kind().code(),
            value_language: value.language().map(|language| language.as_str().to_owned()),
            value_datatype_key: value.datatype_key().map(EntityTypeNameKey::get),
            ..HistoryValueColumns::default()
        };
        let content = match PlacedValue::from(value) {
            PlacedValue::Field(content) => content,
            PlacedValue::Proof(_) => return Err(TemporalStoreInvariant::SealInHistory.into()),
        };
        match content {
            FieldValue::Number(number) => columns.value_number = Some(number),
            FieldValue::String(text) => columns.value_string = Some(text),
            FieldValue::Boolean(flag) => columns.value_boolean = Some(flag),
            FieldValue::Geometry { form, members } => {
                columns.value_geometry = Some(EwkbGeometry::of(&form)?);
                columns.value_geometry_reach = EwkbGeometry::reach_of(&form);
                columns.value_geometry_members = members.map(StoredJson::into_json);
            }
            FieldValue::Object(object) => columns.value_object = Some(object.into_json()),
            FieldValue::DateTime(instant) => columns.value_date_time = Some(instant),
            FieldValue::Uri(uri) => columns.value_uri = Some(String::from(uri)),
        }
        Ok(columns)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_history_value::AttributeHistoryValue,
        error::TemporalStoreError,
        history_value_columns::HistoryValueColumns,
        temporal_store_invariant::TemporalStoreInvariant,
    };
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, reference::NgsiLdReference};
    use andromeda_ngsi_ld_model::temporal::{TemporalValue, text_annotation::TextAnnotation};
    use andromeda_ngsi_ld_number::{
        JsonNumber,
        declared_number::{DeclaredNumber, NonFinite},
    };
    use andromeda_postgresql_attribute_instance::{
        name_keys::NameKeys,
        stored_json::StoredJson,
        stored_number::StoredNumber,
        stored_proof::StoredProof,
        stored_value::{LiteralAnnotation, StoredValue},
    };
    use andromeda_postgresql_stored_values::{entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames, value_kind::ValueKind};
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use serde_json::{json, value::RawValue};

    const INTEGER: &str = "http://www.w3.org/2001/XMLSchema#integer";

    /// Keys holding `xsd:integer` at 12.
    fn keys() -> NameKeys {
        NameKeys::new(
            ResolvedNames::default(),
            ResolvedNames::from_pairs([(INTEGER.parse::<EntityTypeName>().expect("a type name"), EntityTypeNameKey::from(12))]),
        )
    }

    /// The columns `value` is recorded in.
    fn recorded(value: StoredValue) -> HistoryValueColumns {
        HistoryValueColumns::of_attribute(AttributeHistoryValue::Recorded(value)).expect("the value encodes")
    }

    /// How many value fields `columns` sets.
    fn populated(columns: &HistoryValueColumns) -> usize {
        [
            columns.value_number.is_some(),
            columns.value_string.is_some(),
            columns.value_boolean.is_some(),
            columns.value_geometry.is_some(),
            columns.value_object.is_some(),
            columns.value_date_time.is_some(),
            columns.value_uri.is_some(),
        ]
        .into_iter()
        .filter(|set| *set)
        .count()
    }

    #[test]
    fn a_tombstone_declares_the_reserved_kind_and_populates_no_value_field() {
        let deleted_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        let columns = HistoryValueColumns::of_attribute(AttributeHistoryValue::Tombstone { deleted_at }).expect("a tombstone encodes");
        assert_eq!(columns.value_kind, ValueKind::Tombstone.code());
        assert_eq!(columns.deleted_at, Some(deleted_at));
        assert_eq!(populated(&columns), 0);
        assert!(columns.value_datatype_key.is_none() && columns.value_language.is_none());
    }

    #[test]
    fn an_integer_beyond_the_exact_double_range_reaches_the_numeric_column_unchanged() {
        // One above 2^53, which the double format cannot name individually.
        let columns = recorded(StoredValue::Number {
            number: JsonNumber::from(9_007_199_254_740_993_i64),
            datatype: None,
        });
        assert_eq!(columns.value_number, Some(StoredNumber::Finite("9007199254740993".parse().expect("a decimal"))));
    }

    #[test]
    fn a_fractional_number_reaches_the_numeric_column_as_its_exact_decimal() {
        let columns = recorded(StoredValue::Number {
            number: JsonNumber::try_from(-2.5e-4).expect("a finite number"),
            datatype: None,
        });
        assert_eq!(columns.value_number, Some(StoredNumber::Finite("-0.00025".parse().expect("a decimal"))));
    }

    /// A declared number beyond the decimals is held in the number column,
    /// which a write never leaves NULL for a declared number.
    #[test]
    fn a_declared_non_finite_number_reaches_the_numeric_column() {
        for non_finite in [NonFinite::PositiveInfinity, NonFinite::NegativeInfinity, NonFinite::NotANumber] {
            let columns = recorded(StoredValue::DeclaredNumber {
                number: DeclaredNumber::NonFinite(non_finite),
                datatype: None,
            });
            assert_eq!(columns.value_number, Some(StoredNumber::NonFinite(non_finite)));
        }
    }

    #[test]
    fn a_recorded_value_records_no_deletion_instant() {
        assert!(recorded(StoredValue::Boolean { flag: true, datatype: None }).deleted_at.is_none());
    }

    #[test]
    fn every_value_shape_populates_exactly_one_value_field() {
        let instant = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        let point = serde_json::from_value::<Geometry>(json!({"type": "Point", "coordinates": [13.5, 52.25]}))
            .expect("a point")
            .into_parts()
            .0;
        let values = [
            StoredValue::Number {
                number: JsonNumber::from(1),
                datatype: None,
            },
            StoredValue::String {
                text: "text".to_owned(),
                annotation: LiteralAnnotation::Plain,
            },
            StoredValue::Boolean { flag: false, datatype: None },
            StoredValue::Geometry { form: point, members: None },
            StoredValue::Object(StoredJson::new(RawValue::from_string(r#"{"a": 1}"#.to_owned()).expect("a JSON object"))),
            StoredValue::DateTime { instant, datatype: None },
            StoredValue::Uri("urn:ngsi-ld:Entity:target".parse().expect("a valid URI")),
        ];
        for value in values {
            let kind = value.kind();
            let columns = recorded(value);
            assert_eq!(populated(&columns), 1, "{kind:?}");
            assert_eq!(columns.value_kind, kind.code(), "{kind:?}");
        }
    }

    /// A typed literal keeps its datatype as its dictionary key, and a
    /// language-tagged string its tag, in the columns current state holds them
    /// in.
    #[test]
    fn a_literal_keeps_its_datatype_key_and_its_language() {
        let observed = |annotation: TextAnnotation, text: &str| {
            let value = AttributeHistoryValue::of_observation(
                &TemporalValue::String {
                    text: text.to_owned(),
                    annotation,
                },
                &keys(),
            )
            .expect("the value encodes");
            HistoryValueColumns::of_attribute(value).expect("the value encodes")
        };
        let typed = observed(TextAnnotation::Datatype(INTEGER.parse::<NgsiLdReference>().expect("a datatype")), "42");
        assert_eq!((typed.value_datatype_key, typed.value_language), (Some(12), None));
        let tagged = observed(TextAnnotation::Language("es".parse().expect("a language tag")), "hola");
        assert_eq!((tagged.value_datatype_key, tagged.value_language.as_deref()), (None, Some("es")));
    }

    #[test]
    fn a_seal_is_refused_as_a_history_value() {
        let seal = StoredValue::Proof(Box::new(StoredProof {
            entity_id: "urn:ngsi-ld:Store:002".parse().expect("an identifier"),
            entity_type_keys: vec![EntityTypeNameKey::from(3)],
            created: "2025-01-27T21:02:24Z".parse().expect("a DateTime"),
            verification_method: "https://example.edu/issuers/565049#key".parse().expect("a URI"),
            cryptosuite: "eddsa-rdfc-2022".parse().expect("a suite"),
            proof_purpose: "https://w3id.org/security#assertionMethod".parse().expect("a URI"),
            proof_value: "z3XrH3".parse().expect("a signature"),
            members: None,
        }));
        assert!(matches!(
            HistoryValueColumns::of_attribute(AttributeHistoryValue::Recorded(seal)),
            Err(TemporalStoreError::Invariant(TemporalStoreInvariant::SealInHistory))
        ));
    }
}
