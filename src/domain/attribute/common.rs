use crate::{
    domain::{entity::Entity, name::NameBuf, uri::Uri},
    jsonld::{JsonLdExpandable, JsonLdExpandableMacro},
};
use chrono::{DateTime, Utc};
use compact_str::CompactString;
use derive_more::{AsRef, Deref, Display, From};
use geojson::Geometry;
use iref::IriBuf;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::Value;
use serde_with::skip_serializing_none;
use std::{fmt, str::FromStr};
use thiserror::Error;
use urn::Urn;

use crate::domain::attribute::Property;

/// Shared system-managed members for NGSI-LD attributes (ETSI GS CIM 009 clause 5.2).
#[skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonLdExpandableMacro)]
#[serde(rename_all = "camelCase")]
#[jsonld(fragment)]
pub struct AttributeSystemFields {
    /// System-generated creation timestamp.
    #[serde(default)]
    #[jsonld(property = "ngsi:createdAt", typed_value, datatype = "ngsi:DateTime")]
    pub created_at: Option<DateTime<Utc>>,
    /// System-generated deletion timestamp.
    #[serde(default)]
    #[jsonld(property = "ngsi:deletedAt", typed_value, datatype = "ngsi:DateTime")]
    pub deleted_at: Option<DateTime<Utc>>,
    /// URI uniquely identifying an attribute instance.
    #[jsonld(property = "ngsi:instanceId", id_ref)]
    pub instance_id: Option<Uri>,
    /// System-generated last modification timestamp.
    #[serde(default)]
    #[jsonld(property = "ngsi:modifiedAt", typed_value, datatype = "ngsi:DateTime")]
    pub modified_at: Option<DateTime<Utc>>,
}

/// Shared optional members present on all NGSI-LD attribute families (ETSI GS CIM 009 clause 5.2).
#[skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonLdExpandableMacro)]
#[serde(rename_all = "camelCase")]
#[jsonld(fragment)]
pub struct CommonAttributeFields {
    /// Dataset identifier separating multi-instance attributes.
    #[jsonld(property = "ngsi:datasetId", id_ref)]
    pub dataset_id: Option<Urn>,
    /// Attribute expiration timestamp.
    #[serde(default)]
    #[jsonld(property = "ngsi:expiresAt", typed_value, datatype = "ngsi:DateTime")]
    pub expires_at: Option<DateTime<Utc>>,
    /// Optional NGSI-LD data-integrity proof.
    #[jsonld(property = "ngsi:ngsildproof", nested)]
    pub ngsildproof: Option<Box<Property>>,
    /// Observation timestamp for the attribute value.
    #[serde(default)]
    #[jsonld(property = "ngsi:observedAt", typed_value, datatype = "ngsi:DateTime")]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(flatten)]
    #[jsonld(flatten_object)]
    pub system: AttributeSystemFields,
}

/// A value that can appear in `String` or `String[]` NGSI-LD fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany<T> {
    /// A single value.
    One(T),
    /// Multiple values.
    Many(Vec<T>),
}

impl<T> OneOrMany<T> {
    /// Return an iterator over the contained values.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        match self {
            OneOrMany::One(value) => std::slice::from_ref(value).iter(),
            OneOrMany::Many(values) => values.iter(),
        }
    }
}

impl<T> fmt::Display for OneOrMany<T>
where
    T: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OneOrMany::One(value) => write!(f, "{value}"),
            OneOrMany::Many(values) => {
                let joined = values.iter().map(ToString::to_string).collect::<Vec<_>>().join(",");
                f.write_str(&joined)
            }
        }
    }
}

/// Error returned when a compacted NGSI-LD reference string is invalid.
#[derive(Debug, Error)]
pub enum NgsiLdReferenceError {
    /// The value was empty.
    #[error("NGSI-LD reference must not be empty")]
    Empty,
    /// The reference did not match any supported NGSI-LD compacted or expanded form.
    #[error("invalid NGSI-LD reference: {0}")]
    Invalid(String),
}

/// NGSI-LD reference value used by `vocab`, `valueType`, and `objectType` members.
///
/// The compact wire form can be a short name, a compact IRI, or an expanded IRI/URI.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display, AsRef, Deref, From)]
#[serde(try_from = "String", into = "String")]
#[display("{_0}")]
pub struct NgsiLdReference(CompactString);

impl TryFrom<String> for NgsiLdReference {
    type Error = NgsiLdReferenceError;

    fn try_from(value: String) -> Result<NgsiLdReference, NgsiLdReferenceError> {
        validate_reference(&value)?;
        Ok(NgsiLdReference(CompactString::from(value)))
    }
}

impl From<NgsiLdReference> for String {
    fn from(value: NgsiLdReference) -> String {
        value.0.into_string()
    }
}

/// NGSI-LD `Date` lexical form as defined by clause 4.6.3.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display, AsRef, Deref, From)]
#[serde(try_from = "String", into = "String")]
#[display("{_0}")]
pub struct NgsiLdDate(CompactString);

impl TryFrom<String> for NgsiLdDate {
    type Error = NgsiLdValueError;

    fn try_from(value: String) -> Result<NgsiLdDate, NgsiLdValueError> {
        chrono::NaiveDate::parse_from_str(&value, "%Y-%m-%d").map_err(|_| NgsiLdValueError::InvalidDate(value.clone()))?;
        Ok(NgsiLdDate(CompactString::from(value)))
    }
}

impl From<NgsiLdDate> for String {
    fn from(value: NgsiLdDate) -> String {
        value.0.into_string()
    }
}

/// NGSI-LD `Time` lexical form as defined by clause 4.6.3.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display, AsRef, Deref, From)]
#[serde(try_from = "String", into = "String")]
#[display("{_0}")]
pub struct NgsiLdTime(CompactString);

impl TryFrom<String> for NgsiLdTime {
    type Error = NgsiLdValueError;

    fn try_from(value: String) -> Result<NgsiLdTime, NgsiLdValueError> {
        parse_ngsi_ld_time(&value)?;
        Ok(NgsiLdTime(CompactString::from(value)))
    }
}

impl From<NgsiLdTime> for String {
    fn from(value: NgsiLdTime) -> String {
        value.0.into_string()
    }
}

/// Error returned when an NGSI-LD value cannot be coerced into a typed variant.
#[derive(Debug, Error)]
pub enum NgsiLdValueError {
    /// The supplied date literal was invalid.
    #[error("invalid NGSI-LD Date value: {0}")]
    InvalidDate(String),
    /// The supplied time literal was invalid.
    #[error("invalid NGSI-LD Time value: {0}")]
    InvalidTime(String),
}

/// NGSI-LD value family from clause 4.6.3.
///
/// The `Json` variant preserves plain JSON-native values. Typed string forms
/// such as URI, DateTime, Date, and Time can be represented explicitly when
/// they are known semantically, for example after inspecting `valueType`.
#[derive(Debug, Clone, PartialEq)]
pub enum NgsiLdValue {
    /// A plain JSON-native value.
    Json(Value),
    /// A URI value.
    Uri(Uri),
    /// A DateTime value.
    DateTime(DateTime<Utc>),
    /// A Date value.
    Date(NgsiLdDate),
    /// A Time value.
    Time(NgsiLdTime),
    /// A GeoJSON geometry value.
    Geometry(Geometry),
}

impl Serialize for NgsiLdValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            NgsiLdValue::Json(value) => value.serialize(serializer),
            NgsiLdValue::Uri(value) => value.serialize(serializer),
            NgsiLdValue::DateTime(value) => value.serialize(serializer),
            NgsiLdValue::Date(value) => value.serialize(serializer),
            NgsiLdValue::Time(value) => value.serialize(serializer),
            NgsiLdValue::Geometry(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for NgsiLdValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<NgsiLdValue, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if let Some(geometry) = try_geometry(&value) {
            return Ok(NgsiLdValue::Geometry(geometry));
        }
        if let Some(string) = value.as_str() {
            if let Ok(uri) = Uri::from_str(string) {
                return Ok(NgsiLdValue::Uri(uri));
            }
            if let Ok(datetime) = chrono::DateTime::parse_from_rfc3339(string) {
                return Ok(NgsiLdValue::DateTime(datetime.with_timezone(&Utc)));
            }
            if let Ok(date) = NgsiLdDate::try_from(string.to_string()) {
                return Ok(NgsiLdValue::Date(date));
            }
            if let Ok(time) = NgsiLdTime::try_from(string.to_string()) {
                return Ok(NgsiLdValue::Time(time));
            }
        }
        Ok(NgsiLdValue::Json(value))
    }
}

/// `objectList` / `previousObjectList` value as defined by clause 5.2.37.
#[derive(Debug, Clone, PartialEq, Eq, AsRef, Deref, From)]
pub struct RelationshipObjectList(pub Vec<Uri>);

impl Serialize for RelationshipObjectList {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RelationshipObjectList {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<RelationshipObjectList, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let items = value
            .as_array()
            .ok_or_else(|| D::Error::custom("objectList must be an array"))?
            .iter()
            .map(parse_relationship_object_entry)
            .collect::<Result<Vec<_>, _>>()
            .map_err(D::Error::custom)?;
        Ok(RelationshipObjectList(items))
    }
}

impl<'a> IntoIterator for &'a RelationshipObjectList {
    type IntoIter = std::slice::Iter<'a, Uri>;
    type Item = &'a Uri;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl IntoIterator for RelationshipObjectList {
    type IntoIter = std::vec::IntoIter<Uri>;
    type Item = Uri;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// Output-only inline entity payload for `Relationship.entity`.
pub type InlineEntity = OneOrMany<Entity>;

fn parse_relationship_object_entry(value: &Value) -> Result<Uri, String> {
    if let Some(uri) = value.as_str() {
        return Uri::from_str(uri).map_err(|error| error.to_string());
    }

    let object = value
        .as_object()
        .and_then(|object| object.get("object"))
        .and_then(Value::as_str)
        .ok_or_else(|| "relationship object list entries must be strings or {\"object\": ...} objects".to_string())?;
    Uri::from_str(object).map_err(|error| error.to_string())
}

fn try_geometry(value: &Value) -> Option<Geometry> {
    value
        .as_object()
        .and_then(|object| object.get("type").zip(object.get("coordinates")).map(|_pair| value.clone()))
        .and_then(|value| serde_json::from_value(value).ok())
}

fn validate_reference(value: &str) -> Result<(), NgsiLdReferenceError> {
    if value.is_empty() {
        return Err(NgsiLdReferenceError::Empty);
    }
    if Uri::from_str(value).is_ok() || IriBuf::from_str(value).is_ok() || NameBuf::new(value.to_string()).is_ok() {
        return Ok(());
    }
    if let Some((prefix, suffix)) = value.split_once(':')
        && NameBuf::new(prefix.to_string()).is_ok()
        && !suffix.is_empty()
        && !suffix.chars().any(char::is_whitespace)
    {
        return Ok(());
    }
    Err(NgsiLdReferenceError::Invalid(value.to_string()))
}

fn parse_ngsi_ld_time(value: &str) -> Result<(), NgsiLdValueError> {
    let Some(stripped) = value.strip_suffix('Z') else {
        return Err(NgsiLdValueError::InvalidTime(value.to_string()));
    };
    chrono::NaiveTime::parse_from_str(stripped, "%H:%M:%S%.f").map_err(|_| NgsiLdValueError::InvalidTime(value.to_string()))?;
    Ok(())
}

impl JsonLdExpandable for InlineEntity {
    fn to_expanded(&self) -> Value {
        match self {
            OneOrMany::One(entity) => JsonLdExpandable::to_expanded(entity),
            OneOrMany::Many(entities) => Value::Array(entities.iter().map(JsonLdExpandable::to_expanded).collect()),
        }
    }
}

impl JsonLdExpandable for OneOrMany<Uri> {
    fn to_expanded(&self) -> Value {
        Value::Array(self.iter().map(|value| serde_json::json!({ "@id": value.to_string() })).collect())
    }
}

impl JsonLdExpandable for OneOrMany<NgsiLdReference> {
    fn to_expanded(&self) -> Value {
        Value::Array(self.iter().map(|value| serde_json::json!({ "@id": value.to_string() })).collect())
    }
}

impl JsonLdExpandable for OneOrMany<CompactString> {
    fn to_expanded(&self) -> Value {
        Value::Array(self.iter().map(|value| serde_json::json!({ "@id": value.as_str() })).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{NgsiLdDate, NgsiLdReference, NgsiLdTime, NgsiLdValue, OneOrMany, RelationshipObjectList};
    use chrono::{TimeZone, Utc};

    #[test]
    fn ngsi_ld_reference_accepts_short_name() {
        let value = NgsiLdReference::try_from("Temperature".to_string()).expect("short name");
        assert_eq!(value.to_string(), "Temperature");
    }

    #[test]
    fn ngsi_ld_reference_accepts_compact_iri() {
        let value = NgsiLdReference::try_from("schema:Vehicle".to_string()).expect("compact IRI");
        assert_eq!(value.to_string(), "schema:Vehicle");
    }

    #[test]
    fn ngsi_ld_reference_rejects_whitespace() {
        let result = NgsiLdReference::try_from("bad ref".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn ngsi_ld_date_validates_iso_date() {
        let value = NgsiLdDate::try_from("2026-04-12".to_string()).expect("date");
        assert_eq!(value.to_string(), "2026-04-12");
    }

    #[test]
    fn ngsi_ld_time_validates_utc_time() {
        let value = NgsiLdTime::try_from("12:34:56.123Z".to_string()).expect("time");
        assert_eq!(value.to_string(), "12:34:56.123Z");
    }

    #[test]
    fn relationship_object_list_deserializes_string_entries() {
        let value: RelationshipObjectList = serde_json::from_value(serde_json::json!(["urn:ngsi-ld:Entity:1"])).expect("objectList");
        assert_eq!(value.0.len(), 1);
    }

    #[test]
    fn relationship_object_list_deserializes_object_entries() {
        let value: RelationshipObjectList = serde_json::from_value(serde_json::json!([{ "object": "urn:ngsi-ld:Entity:1" }])).expect("objectList");
        assert_eq!(value.0.len(), 1);
    }

    #[test]
    fn one_or_many_iterates_single_and_many() {
        let single = OneOrMany::One(1_u8);
        let many = OneOrMany::Many(vec![1_u8, 2_u8]);
        assert_eq!(single.iter().count(), 1);
        assert_eq!(many.iter().count(), 2);
    }

    #[test]
    fn ngsi_ld_value_serializes_typed_variants() {
        let value = NgsiLdValue::DateTime(Utc.with_ymd_and_hms(2026, 4, 12, 12, 0, 0).unwrap());
        let json = serde_json::to_value(&value).expect("serialize");
        assert_eq!(json, serde_json::json!("2026-04-12T12:00:00Z"));
    }
}
