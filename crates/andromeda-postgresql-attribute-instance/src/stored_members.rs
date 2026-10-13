//! The members of one stored instance, as a row's columns and an element's
//! fields both hold them, and their reading back.
//!
//! A row and a `sub_attribute` element share every member but their position
//! and system times. Reading the members is where a stored instance breaking
//! its invariants (a value in no field or in two, a value in a field its kind
//! does not name, a code no release defines) is refused.

use crate::{
    stored_json::StoredJson,
    stored_value::{LiteralAnnotation, StoredValue},
    sub_attribute::EwkbGeometry,
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
    identifier::{Identifier, IdentifierParseError},
    uri::UriParseError,
};
use andromeda_ngsi_ld_number::{JsonNumber, JsonNumberError};
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, registry::UnknownCompactCode, value_kind::ValueKind};
use andromeda_postgresql_encoding::ewkb::{Ewkb, EwkbError};
use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey};
use andromeda_unit_catalog::unit_code_id::{UnitCodeId, UnitCodeIdOutOfRange};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use langtag::{InvalidLangTag, LangTagBuf};
use serde_json::value::RawValue;
use sqlx::{FromRow, types::Json};
use thiserror::Error;

/// The members of one attribute instance a row and an element both hold.
#[derive(Debug, PartialEq)]
pub struct InstanceMembers {
    /// `attribute_names` key of the attribute.
    pub attribute_name_key: AttributeNameKey,
    /// The `datasetId`, absent on the default instance (clause 4.5.5).
    pub dataset_id: Option<Identifier>,
    /// The attribute family.
    pub attribute_kind: AttributeKind,
    /// The value.
    pub value: StoredValue,
    /// `entity_type_names` key of the `valueType`.
    pub value_type_key: Option<EntityTypeNameKey>,
    /// `entity_type_names` keys of the `objectType` list, in written order.
    pub object_type_keys: Vec<EntityTypeNameKey>,
    /// Catalog identity of the `unitCode`.
    pub unit_code_id: Option<UnitCodeId>,
    /// `observedAt`.
    pub observed_at: Option<DateTime<Utc>>,
    /// `expiresAt` (clause 4.22).
    pub expires_at: Option<DateTime<Utc>>,
    /// The clause 4.5.2.2 seals, in their stored JSON.
    pub proof_seals: Option<StoredJson>,
}

/// The member columns of one stored instance, as an element's fields hold
/// them.
///
/// A row's string and URI each read as one column whichever side of the
/// length split holds it; a writer of a row places them on their side.
#[derive(Debug, FromRow)]
pub struct MemberColumns {
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
    /// `value_string`, or a row's `value_long_text`.
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
    /// `value_uri`, or a row's `value_long_uri`.
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
    /// `expires_at`.
    pub expires_at: Option<DateTime<Utc>>,
}

/// A stored row or `sub_attribute` element that breaks its invariants.
#[derive(Debug, Error)]
pub enum StoredInstanceError {
    /// The attribute or value kind code is one no release defines.
    #[error(transparent)]
    Code(#[from] UnknownCompactCode),
    /// The populated value fields are not exactly the one the value kind
    /// names.
    #[error("the instance's value fields disagree with its value kind {}", .kind.label())]
    ValueFields {
        /// The value kind the instance records.
        kind: ValueKind,
    },
    /// A literal annotation stands beside a value that cannot carry it.
    #[error("the instance carries a datatype or language its value cannot carry")]
    Annotation,
    /// The `datasetId` is not an identifier.
    #[error(transparent)]
    DatasetId(#[from] IdentifierParseError),
    /// The URI is not a URI.
    #[error(transparent)]
    Uri(#[from] UriParseError),
    /// The language tag is not a BCP 47 tag.
    #[error("the instance's language tag is not a BCP 47 tag: {0}")]
    Language(#[source] InvalidLangTag<String>),
    /// The number is not one the JSON number model carries.
    #[error(transparent)]
    Number(#[from] JsonNumberError),
    /// The geometry is not canonical EWKB.
    #[error(transparent)]
    Geometry(#[from] EwkbError),
    /// The unit code identity is out of the catalog's range.
    #[error(transparent)]
    UnitCode(#[from] UnitCodeIdOutOfRange),
}

impl TryFrom<MemberColumns> for InstanceMembers {
    type Error = StoredInstanceError;

    fn try_from(columns: MemberColumns) -> Result<InstanceMembers, StoredInstanceError> {
        let MemberColumns {
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
        } = columns;
        let fields = ValueFields {
            number: value_number,
            string: value_string,
            boolean: value_boolean,
            date_time: value_date_time,
            geometry: value_geometry,
            geometry_members: value_geometry_members,
            object: value_object,
            uri: value_uri,
        };
        let annotation = (
            value_datatype_key.map(EntityTypeNameKey::from),
            value_language.map(LangTagBuf::new).transpose().map_err(StoredInstanceError::Language)?,
        );
        Ok(InstanceMembers {
            attribute_name_key: AttributeNameKey::from(attribute_name_key),
            dataset_id: dataset_id.map(|dataset_id| dataset_id.parse()).transpose()?,
            attribute_kind: AttributeKind::try_from(attribute_kind)?,
            value: fields.into_value(ValueKind::try_from(value_kind)?, annotation)?,
            value_type_key: value_type_key.map(EntityTypeNameKey::from),
            object_type_keys: object_type_keys.unwrap_or_default().into_iter().map(EntityTypeNameKey::from).collect(),
            unit_code_id: unit_code_id.map(UnitCodeId::try_from).transpose()?,
            observed_at,
            expires_at,
            proof_seals: proof_seals.map(StoredJson::from),
        })
    }
}

impl TryFrom<InstanceMembers> for MemberColumns {
    type Error = EwkbError;

    /// The fields `members` occupy, the value in the one field its kind names.
    fn try_from(members: InstanceMembers) -> Result<MemberColumns, EwkbError> {
        let mut columns = MemberColumns {
            attribute_name_key: members.attribute_name_key.get(),
            dataset_id: members.dataset_id.as_ref().map(|dataset_id| dataset_id.as_str().to_owned()),
            attribute_kind: AttributeKind::code(members.attribute_kind),
            value_kind: ValueKind::code(members.value.kind()),
            value_number: None,
            value_string: None,
            value_boolean: None,
            value_date_time: None,
            value_geometry: None,
            value_geometry_members: None,
            value_object: None,
            value_uri: None,
            value_datatype_key: members.value.datatype_key().map(EntityTypeNameKey::get),
            value_language: members.value.language().map(|language| language.as_str().to_owned()),
            value_type_key: members.value_type_key.map(EntityTypeNameKey::get),
            object_type_keys: (!members.object_type_keys.is_empty()).then(|| members.object_type_keys.iter().map(|key| key.get()).collect()),
            proof_seals: members.proof_seals.map(StoredJson::into_json),
            unit_code_id: members.unit_code_id.map(UnitCodeId::get),
            observed_at: members.observed_at,
            expires_at: members.expires_at,
        };
        match members.value {
            // clone: the columns own the decimal they bind, and the number gives up none.
            StoredValue::Number { number, .. } => columns.value_number = Some(number.decimal().clone()),
            StoredValue::String { text, .. } => columns.value_string = Some(text),
            StoredValue::Boolean { flag, .. } => columns.value_boolean = Some(flag),
            StoredValue::DateTime { instant, .. } => columns.value_date_time = Some(instant),
            StoredValue::Geometry { form, members } => {
                columns.value_geometry = Some(EwkbGeometry::of(&form)?);
                columns.value_geometry_members = members.map(StoredJson::into_json);
            }
            StoredValue::Object(object) => columns.value_object = Some(object.into_json()),
            StoredValue::Uri(uri) => columns.value_uri = Some(String::from(uri)),
        }
        Ok(columns)
    }
}

/// The value fields of one instance.
struct ValueFields {
    /// `value_number`.
    number: Option<BigDecimal>,
    /// `value_string`.
    string: Option<String>,
    /// `value_boolean`.
    boolean: Option<bool>,
    /// `value_date_time`.
    date_time: Option<DateTime<Utc>>,
    /// `value_geometry`.
    geometry: Option<EwkbGeometry>,
    /// `value_geometry_members`, which only a geometry carries.
    geometry_members: Option<Json<Box<RawValue>>>,
    /// `value_object`.
    object: Option<Json<Box<RawValue>>>,
    /// `value_uri`.
    uri: Option<String>,
}

impl ValueFields {
    /// The one value the fields hold under `kind`, with the datatype and
    /// language the record carries beside it.
    fn into_value(self, kind: ValueKind, (datatype, language): (Option<EntityTypeNameKey>, Option<LangTagBuf>)) -> Result<StoredValue, StoredInstanceError> {
        let populated = usize::from(self.number.is_some())
            + usize::from(self.string.is_some())
            + usize::from(self.boolean.is_some())
            + usize::from(self.date_time.is_some())
            + usize::from(self.geometry.is_some())
            + usize::from(self.object.is_some())
            + usize::from(self.uri.is_some());
        let misplaced = StoredInstanceError::ValueFields { kind };
        if populated != 1 || (self.geometry_members.is_some() && self.geometry.is_none()) {
            return Err(misplaced);
        }
        let annotated = datatype.is_some() || language.is_some();
        if language.is_some() && !matches!(kind, ValueKind::String) {
            return Err(StoredInstanceError::Annotation);
        }
        let value = match kind {
            ValueKind::Number => StoredValue::Number {
                number: JsonNumber::from_decimal(&self.number.ok_or(misplaced)?)?,
                datatype,
            },
            ValueKind::String => StoredValue::String {
                text: self.string.ok_or(misplaced)?,
                annotation: match (datatype, language) {
                    (None, None) => LiteralAnnotation::Plain,
                    (Some(datatype), None) => LiteralAnnotation::Datatype(datatype),
                    (None, Some(language)) => LiteralAnnotation::Language(language),
                    (Some(_), Some(_)) => return Err(StoredInstanceError::Annotation),
                },
            },
            ValueKind::Boolean => StoredValue::Boolean {
                flag: self.boolean.ok_or(misplaced)?,
                datatype,
            },
            ValueKind::DateTime => StoredValue::DateTime {
                instant: self.date_time.ok_or(misplaced)?,
                datatype,
            },
            ValueKind::Geometry | ValueKind::Object | ValueKind::Uri if annotated => return Err(StoredInstanceError::Annotation),
            ValueKind::Geometry => {
                let ewkb = Ewkb::try_from(self.geometry.ok_or(misplaced)?.0.as_slice())?;
                StoredValue::Geometry {
                    form: Geometry::try_from(&ewkb)?.into_parts().0,
                    members: self.geometry_members.map(StoredJson::from),
                }
            }
            ValueKind::Object => StoredValue::Object(StoredJson::from(self.object.ok_or(misplaced)?)),
            ValueKind::Uri => StoredValue::Uri(self.uri.ok_or(misplaced)?.parse()?),
            ValueKind::Tombstone => return Err(misplaced),
        };
        Ok(value)
    }
}
