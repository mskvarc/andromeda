//! The NGSI-LD value family (ETSI GS CIM 009 clause 4.6.3).

use crate::{
    attribute::{
        date_time::{NgsiLdDate, NgsiLdTime},
        expansion::serde_json_value_to_json_value,
    },
    geometry::Geometry,
    uri::Uri,
};
use chrono::{DateTime, SecondsFormat, Utc};
use jsonld_expandable_core::{JsonValue, ToJsonValue};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::str::FromStr;

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
    /// A GeoJSON geometry value (clause 4.7).
    Geometry(Geometry),
}

// Same unit-variant limitation as `OneOrMany`: `Json(Value)` cannot carry
// `#[default]`, so this one impl stands in for the one `Property` would need.
impl Default for NgsiLdValue {
    fn default() -> NgsiLdValue {
        NgsiLdValue::Json(Value::Null)
    }
}

// Hand-written on both halves: the wire form is an untagged JSON value whose
// variant is decided by inspecting the payload (a string that parses as a URI,
// a DateTime, a Date or a Time; an object that looks like GeoJSON). Serde has
// no derive for "sniff the shape and pick a variant", and `#[serde(untagged)]`
// would try each variant's own Deserialize in declaration order instead.
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
            if let Ok(datetime) = DateTime::parse_from_rfc3339(string) {
                return Ok(NgsiLdValue::DateTime(datetime.with_timezone(&Utc)));
            }
            if let Ok(date) = string.parse::<NgsiLdDate>() {
                return Ok(NgsiLdValue::Date(date));
            }
            if let Ok(time) = string.parse::<NgsiLdTime>() {
                return Ok(NgsiLdValue::Time(time));
            }
        }
        Ok(NgsiLdValue::Json(value))
    }
}

impl<V: JsonValue> ToJsonValue<V> for NgsiLdValue {
    fn to_json_value(&self) -> V {
        match self {
            NgsiLdValue::Json(value) => serde_json_value_to_json_value::<V>(value),
            NgsiLdValue::Uri(uri) => V::string(uri.as_ref()),
            NgsiLdValue::DateTime(dt) => V::from_string(dt.to_rfc3339_opts(SecondsFormat::Millis, true)),
            NgsiLdValue::Date(d) => V::string(d.as_str()),
            NgsiLdValue::Time(t) => V::string(t.as_str()),
            NgsiLdValue::Geometry(g) => g.to_json_value(),
        }
    }
}

/// Decode a JSON value that carries both `type` and `coordinates` as a geometry.
///
/// Anything else is left to the caller so it can fall through to the plain-JSON
/// variant rather than failing outright.
fn try_geometry(value: &Value) -> Option<Geometry> {
    let object = value.as_object()?;
    object.get("type").zip(object.get("coordinates"))?;
    Geometry::from_geojson_value(value).ok()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use crate::attribute::value::NgsiLdValue;
    use chrono::{TimeZone, Utc};

    #[test]
    fn ngsi_ld_value_serializes_typed_variants() {
        let value = NgsiLdValue::DateTime(Utc.with_ymd_and_hms(2026, 4, 12, 12, 0, 0).unwrap());
        let json = serde_json::to_value(&value).expect("serialize");
        pretty_assertions::assert_eq!(json, serde_json::json!("2026-04-12T12:00:00Z"));
    }
}
