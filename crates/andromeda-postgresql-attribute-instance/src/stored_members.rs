//! The members of one stored instance, as a row's columns and an element's
//! fields both hold them, and their reading back.
//!
//! A row and a `sub_attribute` element share every member but their position
//! and system times. Reading the members is where a stored instance breaking
//! its invariants (a value in no field or in two, a value in a field its kind
//! does not name, a code no release defines) is refused.

use crate::{
    stored_json::StoredJson,
    stored_number::StoredNumber,
    stored_proof::ProofFields,
    stored_value::StoredValue,
    stored_value_fields::ValueFields,
    sub_attribute::EwkbGeometry,
};
use andromeda_ngsi_ld_lexicon::{
    identifier::{Identifier, IdentifierParseError},
    time::{date::NgsiLdDateError, time_of_day::NgsiLdTimeError},
    uri::UriParseError,
};
use andromeda_ngsi_ld_number::JsonNumberError;
use andromeda_postgresql_codes::{
    attribute_kind::AttributeKind,
    registry::UnknownCompactCode,
    stored_unit_code::{UnknownStoredUnitCode, stored_unit_code, unit_code_from_stored},
    value_kind::ValueKind,
};
use andromeda_postgresql_encoding::ewkb::EwkbError;
use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey};
use cefact_units::UnitCode;
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
    /// The `unitCode`.
    pub unit_code: Option<UnitCode>,
    /// `observedAt`.
    pub observed_at: Option<DateTime<Utc>>,
    /// `expiresAt` (clause 4.22).
    pub expires_at: Option<DateTime<Utc>>,
}

/// The member columns of one stored instance, as an element's fields hold
/// them, but its `datasetId`.
///
/// A row's string and URI each read as one column whichever side of the
/// length split holds it; a writer of a row places them on their side. The
/// `datasetId` is held apart: a row keys its grain by it, while an element
/// holds it as a nullable field.
#[derive(Debug, FromRow)]
pub struct MemberColumns {
    /// `attribute_name_key`.
    pub attribute_name_key: i32,
    /// `attribute_kind`.
    pub attribute_kind: i16,
    /// `value_kind`.
    pub value_kind: i16,
    /// `value_number`.
    pub value_number: Option<StoredNumber>,
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
    /// The number is not one the JSON number model carries: a non-finite
    /// value, or one past the range it holds.
    #[error(transparent)]
    Number(#[from] JsonNumberError),
    /// The geometry is not canonical EWKB.
    #[error(transparent)]
    Geometry(#[from] EwkbError),
    /// The unit-code identity names no code this build lists.
    #[error(transparent)]
    UnitCode(#[from] UnknownStoredUnitCode),
    /// A `Date` value kind holds text that is not a clause 4.6.3 `Date`.
    #[error("the instance's stored date is not an NGSI-LD Date: {0}")]
    Date(#[from] NgsiLdDateError),
    /// A `Time` value kind holds text that is not a clause 4.6.3 `Time`.
    #[error("the instance's stored time is not an NGSI-LD Time: {0}")]
    Time(#[from] NgsiLdTimeError),
    /// The seal fields of an element disagree with its value kind: a seal
    /// missing a field or holding no value of its member, or a seal field
    /// beside a value of another kind.
    #[error("the instance's data-integrity seal fields disagree with its value kind")]
    SealFields,
}

/// A stored instance whose members a row cannot hold.
#[derive(Debug, Error)]
pub enum RowColumnsError {
    /// The geometry has no EWKB encoding.
    #[error(transparent)]
    Geometry(#[from] EwkbError),
    /// The instance is a data-integrity seal, which only a `sub_attribute`
    /// element holds (clause 4.5.2.2).
    #[error("a data-integrity seal is stored only as a sub-attribute")]
    Seal,
}

impl InstanceMembers {
    /// The members a row's `columns` hold, of the instance `dataset_id`
    /// names. A row holds no data-integrity seal.
    ///
    /// # Errors
    ///
    /// Returns [`StoredInstanceError`] when the columns break an invariant of
    /// a stored instance.
    pub fn read(dataset_id: Option<Identifier>, columns: MemberColumns) -> Result<InstanceMembers, StoredInstanceError> {
        InstanceMembers::read_element(dataset_id, columns, ProofFields::default())
    }

    /// The members an element's `columns` and seal fields `proof` hold, of
    /// the instance `dataset_id` names.
    ///
    /// # Errors
    ///
    /// Returns [`StoredInstanceError`] when the fields break an invariant of a
    /// stored instance.
    pub fn read_element(dataset_id: Option<Identifier>, columns: MemberColumns, proof: ProofFields) -> Result<InstanceMembers, StoredInstanceError> {
        let MemberColumns {
            attribute_name_key,
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
            dataset_id,
            attribute_kind: AttributeKind::try_from(attribute_kind)?,
            value: match ValueKind::try_from(value_kind)? {
                ValueKind::DataIntegrityProof => {
                    if !fields.is_empty() || annotation.0.is_some() || annotation.1.is_some() {
                        return Err(StoredInstanceError::ValueFields {
                            kind: ValueKind::DataIntegrityProof,
                        });
                    }
                    StoredValue::Proof(Box::new(proof.read()?))
                }
                kind @ (ValueKind::Tombstone
                | ValueKind::Number
                | ValueKind::String
                | ValueKind::Boolean
                | ValueKind::Geometry
                | ValueKind::Object
                | ValueKind::DateTime
                | ValueKind::Uri
                | ValueKind::Date
                | ValueKind::Time
                | ValueKind::DeclaredNumber
                | ValueKind::DeclaredBoolean) => {
                    if !proof.is_empty() {
                        return Err(StoredInstanceError::SealFields);
                    }
                    fields.into_value(kind, annotation)?
                }
            },
            value_type_key: value_type_key.map(EntityTypeNameKey::from),
            object_type_keys: object_type_keys.unwrap_or_default().into_iter().map(EntityTypeNameKey::from).collect(),
            unit_code: unit_code_id.map(unit_code_from_stored).transpose()?,
            observed_at,
            expires_at,
        })
    }

    /// The `datasetId` of the instance and the columns its other members
    /// occupy in a row, the value in the one column its kind names.
    ///
    /// # Errors
    ///
    /// Returns [`RowColumnsError::Geometry`] when the geometry has no EWKB
    /// encoding, and [`RowColumnsError::Seal`] for a data-integrity seal.
    pub fn into_columns(self) -> Result<(Option<Identifier>, MemberColumns), RowColumnsError> {
        match self.into_element_columns()? {
            (dataset_id, columns, proof) if proof.is_empty() => Ok((dataset_id, columns)),
            (_, _, _) => Err(RowColumnsError::Seal),
        }
    }

    /// The `datasetId` of the instance, the fields its other members occupy
    /// in an element, the value in the one field its kind names, and the seal
    /// fields, all NULL but for a data-integrity seal.
    ///
    /// # Errors
    ///
    /// Returns [`EwkbError`] when the geometry has no EWKB encoding.
    pub fn into_element_columns(self) -> Result<(Option<Identifier>, MemberColumns, ProofFields), EwkbError> {
        let mut columns = MemberColumns {
            attribute_name_key: self.attribute_name_key.get(),
            attribute_kind: AttributeKind::code(self.attribute_kind),
            value_kind: ValueKind::code(self.value.kind()),
            value_number: None,
            value_string: None,
            value_boolean: None,
            value_date_time: None,
            value_geometry: None,
            value_geometry_members: None,
            value_object: None,
            value_uri: None,
            value_datatype_key: self.value.datatype_key().map(EntityTypeNameKey::get),
            value_language: self.value.language().map(|language| language.as_str().to_owned()),
            value_type_key: self.value_type_key.map(EntityTypeNameKey::get),
            object_type_keys: (!self.object_type_keys.is_empty()).then(|| self.object_type_keys.iter().map(|key| key.get()).collect()),
            unit_code_id: self.unit_code.map(stored_unit_code),
            observed_at: self.observed_at,
            expires_at: self.expires_at,
        };
        match self.value {
            StoredValue::Number { number, .. } => columns.value_number = Some(StoredNumber::from(number)),
            StoredValue::String { text, .. } => columns.value_string = Some(text),
            StoredValue::Boolean { flag, .. } | StoredValue::DeclaredBoolean { flag, .. } => columns.value_boolean = Some(flag),
            StoredValue::DeclaredNumber { number, .. } => columns.value_number = Some(StoredNumber::from(number)),
            StoredValue::DateTime { instant, .. } => columns.value_date_time = Some(instant),
            StoredValue::Date { date, .. } => columns.value_string = Some(date.to_string()),
            StoredValue::Time { time, .. } => columns.value_string = Some(time.to_string()),
            StoredValue::Geometry { form, members } => {
                columns.value_geometry = Some(EwkbGeometry::of(&form)?);
                columns.value_geometry_members = members.map(StoredJson::into_json);
            }
            StoredValue::Object(object) => columns.value_object = Some(object.into_json()),
            StoredValue::Uri(uri) => columns.value_uri = Some(String::from(uri)),
            StoredValue::Proof(proof) => return Ok((self.dataset_id, columns, ProofFields::from(*proof))),
        }
        Ok((self.dataset_id, columns, ProofFields::default()))
    }
}
