//! The value fields of one stored instance, and the one value they hold.
//!
//! A row and an element hold a value in one typed field per scalar kind and in
//! `value_object` for every other value. Reading them is where a value in no
//! field or in two, or in a field its kind does not name, is refused.

use crate::{
    stored_json::StoredJson,
    stored_members::StoredInstanceError,
    stored_number::StoredNumber,
    stored_value::{LiteralAnnotation, StoredValue},
    sub_attribute::EwkbGeometry,
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_number::{JsonNumber, declared_number::DeclaredNumber};
use andromeda_postgresql_codes::value_kind::ValueKind;
use andromeda_postgresql_encoding::ewkb::Ewkb;
use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
use chrono::{DateTime, Utc};
use langtag::LangTagBuf;
use serde_json::value::RawValue;
use sqlx::types::Json;

/// The value fields of one instance.
pub(crate) struct ValueFields {
    /// `value_number`.
    pub(crate) number: Option<StoredNumber>,
    /// `value_string`.
    pub(crate) string: Option<String>,
    /// `value_boolean`.
    pub(crate) boolean: Option<bool>,
    /// `value_date_time`.
    pub(crate) date_time: Option<DateTime<Utc>>,
    /// `value_geometry`.
    pub(crate) geometry: Option<EwkbGeometry>,
    /// `value_geometry_members`, which only a geometry carries.
    pub(crate) geometry_members: Option<Json<Box<RawValue>>>,
    /// `value_object`.
    pub(crate) object: Option<Json<Box<RawValue>>>,
    /// `value_uri`.
    pub(crate) uri: Option<String>,
}

impl ValueFields {
    /// Whether no value field is populated, as on a data-integrity seal.
    pub(crate) const fn is_empty(&self) -> bool {
        self.number.is_none()
            && self.string.is_none()
            && self.boolean.is_none()
            && self.date_time.is_none()
            && self.geometry.is_none()
            && self.geometry_members.is_none()
            && self.object.is_none()
            && self.uri.is_none()
    }

    /// The one value the fields hold under `kind`, with the datatype and
    /// language the record carries beside it.
    pub(crate) fn into_value(
        self,
        kind: ValueKind,
        (datatype, language): (Option<EntityTypeNameKey>, Option<LangTagBuf>),
    ) -> Result<StoredValue, StoredInstanceError> {
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
                number: JsonNumber::try_from(self.number.ok_or(misplaced)?)?,
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
            ValueKind::DeclaredNumber => StoredValue::DeclaredNumber {
                number: DeclaredNumber::try_from(self.number.ok_or(misplaced)?)?,
                datatype,
            },
            ValueKind::DeclaredBoolean => StoredValue::DeclaredBoolean {
                flag: self.boolean.ok_or(misplaced)?,
                datatype,
            },
            ValueKind::DateTime => StoredValue::DateTime {
                instant: self.date_time.ok_or(misplaced)?,
                datatype,
            },
            ValueKind::Date => StoredValue::Date {
                date: self.string.ok_or(misplaced)?.parse()?,
                datatype,
            },
            ValueKind::Time => StoredValue::Time {
                time: self.string.ok_or(misplaced)?.parse()?,
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
            ValueKind::Tombstone | ValueKind::DataIntegrityProof => return Err(misplaced),
        };
        Ok(value)
    }
}
