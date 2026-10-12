//! One element of `current_attributes.sub_attributes`, and the `sub_attribute`
//! composite it travels as.
//!
//! [`SubAttributeElement`] is the typed element: one value by construction.
//! [`SubAttributeRecord`] is the composite field for field, in the type's
//! declaration order, encoded and decoded in the binary protocol by the
//! `sqlx` derive. Converting a record into an element is where a stored
//! element breaking its invariants is refused, as a row's members are.

use crate::{
    stored_json::StoredJson,
    stored_members::{InstanceMembers, MemberColumns, StoredInstanceError},
    stored_value::StoredValue,
};
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, value_kind::ValueKind};
use andromeda_postgresql_encoding::ewkb::{EwkbError, encode};
use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde_json::value::RawValue;
use sqlx::{
    Decode,
    Encode,
    Postgres,
    Type,
    encode::IsNull,
    error::BoxDynError,
    postgres::{PgArgumentBuffer, PgTypeInfo, PgValueRef},
    types::Json,
};

/// One sub-attribute instance inside its top-level instance's row.
#[derive(Debug, PartialEq)]
pub struct SubAttributeElement {
    /// 0 for a sub-attribute of the top-level instance, else the 1-based
    /// array position of the element it belongs to, which precedes it.
    pub parent: i32,
    /// The element's members.
    pub members: InstanceMembers,
    /// `createdAt` (clause 4.8), when stamped.
    pub created_at: Option<DateTime<Utc>>,
    /// `modifiedAt` (clause 4.8), when stamped.
    pub modified_at: Option<DateTime<Utc>>,
}

/// A geometry in the binary form of `PostGIS` `geometry`, which is EWKB.
#[derive(Debug)]
pub struct EwkbGeometry(pub(crate) Vec<u8>);

// Hand-written: `geometry` is a `PostGIS` type `sqlx` knows only by name, and
// its binary send and receive form is the EWKB the store already builds, so
// the bytes pass through unchanged.
impl Type<Postgres> for EwkbGeometry {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("geometry")
    }
}

// Hand-written: see the `Type` implementation.
impl Encode<'_, Postgres> for EwkbGeometry {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        buf.extend_from_slice(&self.0);
        Ok(IsNull::No)
    }
}

// Hand-written: see the `Type` implementation.
impl Decode<'_, Postgres> for EwkbGeometry {
    fn decode(value: PgValueRef<'_>) -> Result<EwkbGeometry, BoxDynError> {
        Ok(EwkbGeometry(value.as_bytes()?.to_vec()))
    }
}

/// The `sub_attribute` composite, field for field in declaration order.
#[derive(Debug, Type)]
#[sqlx(type_name = "sub_attribute")]
pub struct SubAttributeRecord {
    /// `parent`.
    pub parent: i32,
    /// `attribute_name_key`.
    pub attribute_name_key: i32,
    /// `dataset_id`.
    pub dataset_id: Option<String>,
    /// `attribute_kind`.
    pub attribute_kind: i16,
    /// `value_kind`.
    pub value_kind: i16,
    /// `value_number`.
    pub value_number: Option<BigDecimal>,
    /// `value_string`.
    pub value_string: Option<String>,
    /// `value_boolean`.
    pub value_boolean: Option<bool>,
    /// `value_date_time`.
    pub value_date_time: Option<DateTime<Utc>>,
    /// `value_geometry`.
    pub value_geometry: Option<EwkbGeometry>,
    /// `value_geometry_members`.
    pub value_geometry_members: Option<Json<Box<RawValue>>>,
    /// `value_object`.
    pub value_object: Option<Json<Box<RawValue>>>,
    /// `value_uri`.
    pub value_uri: Option<String>,
    /// `value_datatype_key`.
    pub value_datatype_key: Option<i32>,
    /// `value_language`.
    pub value_language: Option<String>,
    /// `value_type_key`.
    pub value_type_key: Option<i32>,
    /// `object_type_keys`.
    pub object_type_keys: Option<Vec<i32>>,
    /// `proof_seals`.
    pub proof_seals: Option<Json<Box<RawValue>>>,
    /// `unit_code_id`.
    pub unit_code_id: Option<i16>,
    /// `observed_at`.
    pub observed_at: Option<DateTime<Utc>>,
    /// `created_at`.
    pub created_at: Option<DateTime<Utc>>,
    /// `modified_at`.
    pub modified_at: Option<DateTime<Utc>>,
    /// `expires_at`.
    pub expires_at: Option<DateTime<Utc>>,
}

/// The `sub_attribute_tree` composite: one row's elements as one value, which
/// a statement writing many rows binds one per row.
#[derive(Debug, Type)]
#[sqlx(type_name = "sub_attribute_tree")]
pub struct SubAttributeTree {
    /// `elements`, in the order the row holds them.
    pub elements: Vec<SubAttributeRecord>,
}

impl TryFrom<SubAttributeElement> for SubAttributeRecord {
    type Error = EwkbError;

    fn try_from(element: SubAttributeElement) -> Result<SubAttributeRecord, EwkbError> {
        let SubAttributeElement {
            parent,
            members,
            created_at,
            modified_at,
        } = element;
        let value_kind = ValueKind::code(members.value.kind());
        let value_datatype_key = members.value.datatype_key().map(EntityTypeNameKey::get);
        let value_language = members.value.language().map(|language| language.as_str().to_owned());
        let mut record = SubAttributeRecord {
            parent,
            attribute_name_key: members.attribute_name_key.get(),
            dataset_id: members.dataset_id.as_ref().map(|dataset_id| dataset_id.as_str().to_owned()),
            attribute_kind: AttributeKind::code(members.attribute_kind),
            value_kind,
            value_number: None,
            value_string: None,
            value_boolean: None,
            value_date_time: None,
            value_geometry: None,
            value_geometry_members: None,
            value_object: None,
            value_uri: None,
            value_datatype_key,
            value_language,
            value_type_key: members.value_type_key.map(EntityTypeNameKey::get),
            object_type_keys: (!members.object_type_keys.is_empty()).then(|| members.object_type_keys.iter().map(|key| key.get()).collect()),
            proof_seals: members.proof_seals.map(StoredJson::into_json),
            unit_code_id: members.unit_code_id.map(UnitCodeId::get),
            observed_at: members.observed_at,
            created_at,
            modified_at,
            expires_at: members.expires_at,
        };
        match members.value {
            // clone: the record owns the decimal it binds, and the number gives up none.
            StoredValue::Number { number, .. } => record.value_number = Some(number.decimal().clone()),
            StoredValue::String { text, .. } => record.value_string = Some(text),
            StoredValue::Boolean { flag, .. } => record.value_boolean = Some(flag),
            StoredValue::DateTime { instant, .. } => record.value_date_time = Some(instant),
            StoredValue::Geometry { form, members } => {
                record.value_geometry = Some(EwkbGeometry(encode(&form)?.to_vec()));
                record.value_geometry_members = members.map(StoredJson::into_json);
            }
            StoredValue::Object(object) => record.value_object = Some(object.into_json()),
            StoredValue::Uri(uri) => record.value_uri = Some(String::from(uri)),
        }
        Ok(record)
    }
}

impl TryFrom<SubAttributeRecord> for SubAttributeElement {
    type Error = StoredInstanceError;

    fn try_from(record: SubAttributeRecord) -> Result<SubAttributeElement, StoredInstanceError> {
        let SubAttributeRecord {
            parent,
            attribute_name_key,
            dataset_id,
            attribute_kind,
            value_kind,
            value_number,
            value_string,
            value_boolean,
            value_date_time,
            value_geometry,
            value_geometry_members,
            value_object,
            value_uri,
            value_datatype_key,
            value_language,
            value_type_key,
            object_type_keys,
            proof_seals,
            unit_code_id,
            observed_at,
            created_at,
            modified_at,
            expires_at,
        } = record;
        let columns = MemberColumns {
            attribute_name_key,
            dataset_id,
            attribute_kind,
            value_kind,
            value_number,
            value_string,
            value_boolean,
            value_date_time,
            value_geometry,
            value_geometry_members,
            value_object,
            value_uri,
            value_datatype_key,
            value_language,
            value_type_key,
            object_type_keys,
            proof_seals,
            unit_code_id,
            observed_at,
            expires_at,
        };
        Ok(SubAttributeElement {
            parent,
            members: InstanceMembers::try_from(columns)?,
            created_at,
            modified_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        stored_json::StoredJson,
        stored_members::{InstanceMembers, StoredInstanceError},
        stored_value::StoredValue,
        sub_attribute::{SubAttributeElement, SubAttributeRecord},
    };
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use andromeda_postgresql_codes::{attribute_kind::AttributeKind, value_kind::ValueKind};
    use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey};
    use bigdecimal::BigDecimal;
    use chrono::{TimeZone, Utc};
    use serde_json::{json, value::to_raw_value};

    /// A stored record of a Property element holding the number 1.
    fn number_record() -> SubAttributeRecord {
        SubAttributeRecord {
            parent: 0,
            attribute_name_key: 1,
            dataset_id: None,
            attribute_kind: AttributeKind::code(AttributeKind::Property),
            value_kind: ValueKind::code(ValueKind::Number),
            value_number: Some(BigDecimal::from(1)),
            value_string: None,
            value_boolean: None,
            value_date_time: None,
            value_geometry: None,
            value_geometry_members: None,
            value_object: None,
            value_uri: None,
            value_datatype_key: None,
            value_language: None,
            value_type_key: None,
            object_type_keys: None,
            proof_seals: None,
            unit_code_id: None,
            observed_at: None,
            created_at: None,
            modified_at: None,
            expires_at: None,
        }
    }

    #[test]
    fn an_element_with_two_values_is_an_assembly_error() {
        let mut record = number_record();
        record.value_string = Some("a".to_owned());
        assert!(matches!(
            SubAttributeElement::try_from(record),
            Err(StoredInstanceError::ValueFields { kind: ValueKind::Number })
        ));
    }

    #[test]
    fn an_element_whose_value_is_in_another_field_is_an_assembly_error() {
        let mut record = number_record();
        record.value_number = None;
        record.value_boolean = Some(true);
        assert!(matches!(
            SubAttributeElement::try_from(record),
            Err(StoredInstanceError::ValueFields { kind: ValueKind::Number })
        ));
    }

    #[test]
    fn a_language_beside_a_number_is_an_assembly_error() {
        let mut record = number_record();
        record.value_language = Some("en".to_owned());
        assert!(matches!(SubAttributeElement::try_from(record), Err(StoredInstanceError::Annotation)));
    }

    #[test]
    fn the_element_round_trips_through_its_record() {
        let instant = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).single().expect("a valid instant");
        let full = SubAttributeElement {
            parent: 3,
            members: InstanceMembers {
                attribute_name_key: AttributeNameKey::from(7),
                dataset_id: Some("urn:ngsi-ld:Dataset:a".parse().expect("a datasetId")),
                attribute_kind: AttributeKind::GeoProperty,
                value: StoredValue::Geometry {
                    form: serde_json::from_value::<Geometry>(json!({"type": "Point", "coordinates": [13.5, 52.25, 34.0]}))
                        .expect("a point")
                        .into_parts()
                        .0,
                    members: Some(StoredJson::new(
                        to_raw_value(&json!({"https://ex.org/title": [{"@value": "x"}]})).expect("JSON"),
                    )),
                },
                value_type_key: Some(EntityTypeNameKey::from(3)),
                object_type_keys: vec![EntityTypeNameKey::from(4), EntityTypeNameKey::from(5)],
                unit_code_id: Some(1_i16.try_into().expect("a unit code identity")),
                observed_at: Some(instant),
                expires_at: Some(instant),
                proof_seals: Some(StoredJson::new(
                    to_raw_value(&json!({"https://uri.etsi.org/ngsi-ld/entityIdSealed": [{"@value": "x"}]})).expect("JSON"),
                )),
            },
            created_at: Some(instant),
            modified_at: Some(instant),
        };
        let empty = || SubAttributeElement {
            parent: 0,
            members: InstanceMembers {
                attribute_name_key: AttributeNameKey::from(7),
                dataset_id: None,
                attribute_kind: AttributeKind::Property,
                value: StoredValue::Boolean { flag: false, datatype: None },
                value_type_key: None,
                object_type_keys: Vec::new(),
                unit_code_id: None,
                observed_at: None,
                expires_at: None,
                proof_seals: None,
            },
            created_at: None,
            modified_at: None,
        };
        let through = |element: SubAttributeElement| {
            SubAttributeElement::try_from(SubAttributeRecord::try_from(element).expect("the element encodes")).expect("the record decodes")
        };
        let full_debug = format!("{full:?}");
        pretty_assertions::assert_eq!(format!("{:?}", through(full)), full_debug);
        pretty_assertions::assert_eq!(through(empty()), empty());
        let record = SubAttributeRecord::try_from(empty()).expect("the element encodes");
        assert!(record.object_type_keys.is_none() && record.value_geometry_members.is_none() && record.proof_seals.is_none());
    }
}
