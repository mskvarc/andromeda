//! The NGSI-LD value family (ETSI GS CIM 009 clause 4.6.3).

use crate::attribute::expansion::serde_json_value_to_json_value;
use andromeda_ngsi_ld_geometry::{geojson::GeometryError, geometry::Geometry};
use andromeda_ngsi_ld_lexicon::{ngsi_ld_date::NgsiLdDate, ngsi_ld_time::NgsiLdTime, rfc3339::whole_seconds, uri::Uri};
use chrono::{DateTime, Utc};
use jsonld_expandable_core::{JsonValue, ToJsonValue};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeserializeError};
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
        if let Some(geometry) = try_geometry(&value).map_err(DeserializeError::custom)? {
            return Ok(NgsiLdValue::Geometry(geometry));
        }
        if let Some(string) = value.as_str() {
            if let Ok(uri) = Uri::from_str(string) {
                return Ok(NgsiLdValue::Uri(uri));
            }
            if let Ok(datetime) = DateTime::parse_from_rfc3339(string) {
                return Ok(NgsiLdValue::DateTime(datetime.with_timezone(&Utc)));
            }
            // `Date` and `Time` are deliberately not sniffed here, unlike
            // `DateTime` above. `NgsiLdTime` canonicalises through `Display`
            // (clause 4.6.3 admits any offset and fractional width, and it
            // re-renders to one fixed form), so sniffing it would rewrite an
            // untyped string Property value the client wrote no differently
            // from `"12:00:00Z"` into `"12:00:00.000000Z"` — a value change
            // with no `@type: Time` anywhere to justify it. The two write
            // paths that mean `Date`/`Time` say so explicitly through
            // `valueType`/`@type`, which reaches these variants without going
            // through this sniffing ladder at all.
        }
        Ok(NgsiLdValue::Json(value))
    }
}

impl<V: JsonValue> ToJsonValue<V> for NgsiLdValue {
    fn to_json_value(&self) -> V {
        match self {
            NgsiLdValue::Json(value) => serde_json_value_to_json_value::<V>(value),
            NgsiLdValue::Uri(uri) => V::string(uri.as_ref()),
            NgsiLdValue::DateTime(dt) => V::from_string(whole_seconds(*dt)),
            // Both render in the canonical clause 4.6.3 form their types define,
            // which is the same text they are stored and compared as.
            NgsiLdValue::Date(d) => V::from_string(d.to_string()),
            NgsiLdValue::Time(t) => V::from_string(t.to_string()),
            NgsiLdValue::Geometry(g) => g.to_json_value(),
        }
    }
}

/// Decode a JSON value that carries both `type` and `coordinates` as a geometry.
///
/// `Ok(None)` says the value is not GeoJSON-shaped at all, which leaves the
/// caller free to fall through to the plain-JSON variant. A value that *is*
/// GeoJSON-shaped and still fails to decode is an error rather than a
/// fall-through: silently demoting, say, a Point whose position carries four
/// ordinates to an ordinary Property would store an unusable geometry under a
/// GeoProperty and hide a clause 5.5.4 `BadRequestData` from the client.
///
/// # Errors
///
/// Returns [`GeometryError::Position`] when a position carries an ordinate
/// count IETF RFC 7946 section 3.1.1 gives no meaning to, and
/// [`GeometryError::GeojsonValue`] when the `type`/`coordinates` pair is not a
/// GeoJSON geometry object.
fn try_geometry(value: &Value) -> Result<Option<Geometry>, GeometryError> {
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    if object.get("type").zip(object.get("coordinates")).is_none() {
        return Ok(None);
    }
    Geometry::from_geojson_value(value).map(Some)
}

#[cfg(test)]
mod tests {
    use crate::attribute::value::{NgsiLdValue, try_geometry};
    use andromeda_ngsi_ld_geometry::{dimension::GeometryDimension, geojson::GeometryError, position::PositionError};
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    #[test]
    fn ngsi_ld_value_serializes_typed_variants() {
        let value = NgsiLdValue::DateTime(Utc.with_ymd_and_hms(2026, 4, 12, 12, 0, 0).single().expect("a valid instant"));
        let json = serde_json::to_value(&value).expect("serialize");
        pretty_assertions::assert_eq!(json, json!("2026-04-12T12:00:00Z"));
    }

    #[test]
    fn a_two_dimensional_geojson_object_decodes_as_a_geometry() {
        let geometry = try_geometry(&json!({"type": "Point", "coordinates": [13.38, 52.52]}))
            .expect("a well-formed Point decodes")
            .expect("a GeoJSON-shaped value yields a geometry");
        pretty_assertions::assert_eq!(geometry.dimension(), GeometryDimension::TwoD);
    }

    #[test]
    fn a_three_dimensional_geojson_object_decodes_with_its_altitude() {
        // Clause 4.7 adopts IETF RFC 7946 whole, altitude included.
        let geometry = try_geometry(&json!({"type": "Point", "coordinates": [13.38, 52.52, 34.5]}))
            .expect("an elevated Point decodes")
            .expect("a GeoJSON-shaped value yields a geometry");
        pretty_assertions::assert_eq!(geometry.dimension(), GeometryDimension::ThreeD);
    }

    #[test]
    fn a_geojson_object_whose_position_carries_four_ordinates_is_an_error() {
        // Refusing here is what stops a clause 5.5.4 BadRequestData from being
        // silently demoted to an ordinary Property carrying raw JSON.
        let error = try_geometry(&json!({"type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0]})).expect_err("four ordinates have no meaning");
        assert!(
            matches!(error, GeometryError::Position(PositionError::TooManyOrdinates { ordinates: 4 })),
            "got {error:?}"
        );
    }

    #[test]
    fn a_value_that_is_not_geojson_shaped_is_left_to_the_plain_json_variant() {
        pretty_assertions::assert_eq!(try_geometry(&json!({"type": "Point"})).expect("no coordinates member").is_none(), true);
        pretty_assertions::assert_eq!(try_geometry(&json!("a string")).expect("not an object").is_none(), true);
        pretty_assertions::assert_eq!(try_geometry(&json!({"speed": 42})).expect("an ordinary object").is_none(), true);
    }

    #[test]
    fn deserializing_a_four_ordinate_geometry_fails_rather_than_demoting_to_json() {
        let error = serde_json::from_value::<NgsiLdValue>(json!({"type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0]}))
            .expect_err("the value family must refuse an unusable geometry");
        assert!(error.to_string().contains("4 ordinates"), "got {error}");
    }
}
