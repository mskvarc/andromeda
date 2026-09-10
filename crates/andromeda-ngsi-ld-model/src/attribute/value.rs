//! The NGSI-LD value family (ETSI GS CIM 009 clause 4.6.3).

use crate::attribute::expansion::serde_json_value_to_json_value;
use andromeda_ngsi_ld_geometry::{geojson::GeometryError, geometry::Geometry};
use jsonld_expandable_core::{JsonValue, ToJsonValue};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeserializeError};
use serde_json::Value;

/// NGSI-LD value family from clause 4.6.3.
///
/// Clause 4.6.4 requires that "implementations shall preserve the
/// representation of the content of the values provided by the context
/// information providers and return the original content when replying to
/// context consumption requests", so this type is the client's own document and
/// carries the value in the spelling the client wrote it in. The one value that
/// is not carried as raw JSON is a `GeoJSON` geometry, and that is not a
/// re-spelling: clause 4.7 adopts IETF RFC 7946 whole and a geometry that is not
/// one is a clause 5.5.4 `BadRequestData` the decode has to be able to raise.
///
/// What a value *is* under clause 4.6.3 is a separate question from how it is
/// written, and it is decided once, in the lexicon, where the write projection
/// and the clause 4.9 operand parser both read it.
#[derive(Debug, Clone, PartialEq)]
pub enum NgsiLdValue {
    /// A plain JSON-native value, in the spelling the client wrote.
    Json(Value),
    /// A `GeoJSON` geometry value (clause 4.7).
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
// variant is decided by inspecting the payload for a GeoJSON geometry object.
// Serde has no derive for "sniff the shape and pick a variant", and
// `#[serde(untagged)]` would try each variant's own Deserialize in declaration
// order instead.
impl Serialize for NgsiLdValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            NgsiLdValue::Json(value) => value.serialize(serializer),
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
        // No string value is sniffed into a typed variant here, and the reason
        // is the same for all of them: every such type canonicalises through
        // `Display`, so recognising one rewrites the spelling the client wrote.
        // Clause 4.6.4 forbids exactly that — "implementations shall preserve
        // the representation of the content of the values provided by the
        // context information providers and return the original content when
        // replying to context consumption requests" — and nothing anywhere in
        // clause 4.6 licenses a broker to substitute one conformant spelling for
        // another. Sniffing a URI rewrote `URN:NGSI-LD:Building:001` into
        // `urn:ngsi-ld:Building:001` and lowercased the scheme, host and default
        // port of a URL value; sniffing a `DateTime` through a looser RFC 3339
        // parse than clause 4.6.3 admits rewrote `2024-01-01T01:00:00+01:00`,
        // which the clause makes a plain string, into `2024-01-01T00:00:00Z`.
        //
        // What the value *is* under clause 4.6.3 is decided once, by
        // `andromeda_ngsi_ld_lexicon::time_value::TimeValue`, where the write
        // projection and the clause 4.9 `q` operand parser both read it, and
        // that is what makes a stored timestamp findable by the timestamp that
        // wrote it. Normalisation of an identifier belongs where the
        // specification declares one — an Entity `id`, a Relationship `object`,
        // a `datasetId` — not on a Property value that merely resembles one.
        Ok(NgsiLdValue::Json(value))
    }
}

impl<V: JsonValue> ToJsonValue<V> for NgsiLdValue {
    fn to_json_value(&self) -> V {
        match self {
            NgsiLdValue::Json(value) => serde_json_value_to_json_value::<V>(value),
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
/// `GeoProperty` and hide a clause 5.5.4 `BadRequestData` from the client.
///
/// # Errors
///
/// Returns [`GeometryError::Position`] when a position carries an ordinate
/// count IETF RFC 7946 section 3.1.1 gives no meaning to, and
/// [`GeometryError::GeojsonValue`] when the `type`/`coordinates` pair is not a
/// `GeoJSON` geometry object.
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
    use serde_json::json;

    /// Clause 4.6.4: "implementations shall preserve the representation of the
    /// content of the values provided by the context information providers and
    /// return the original content when replying to context consumption
    /// requests." A Property value that happens to parse as a URI is still a
    /// value and not an identifier — clause 4.6.3 declares no datatype for it,
    /// clause 4.6.2 declares one only for the members the specification names as
    /// identifiers — so the URI equivalence rules that lowercase a URN's scheme
    /// and NID, or a URL's scheme, host and default port, have nothing to
    /// operate on here. Recognising one rewrote the client's text.
    #[test]
    fn a_value_that_parses_as_a_uri_is_served_back_in_the_spelling_the_client_wrote() {
        for literal in [
            "URN:NGSI-LD:Building:001",
            "urn:ngsi-ld:Building:001",
            "HTTPS://Example.COM:443/Path",
            "https://example.com",
            "mailto:someone@example.org",
        ] {
            let value: NgsiLdValue = serde_json::from_value(json!(literal)).expect("a string value");
            pretty_assertions::assert_eq!(value, NgsiLdValue::Json(json!(literal)), "{literal}");
            pretty_assertions::assert_eq!(serde_json::to_value(&value).expect("serialize"), json!(literal), "{literal}");
        }
    }

    /// Clause 4.6.3 admits several spellings of one time value — a fraction of
    /// "one or more fractional digits, up to a maximum of six", and "in
    /// requests, also a comma instead of a decimal point" — and licenses no
    /// broker to re-render the conformant one a client chose. The document
    /// representation therefore carries the text; what the value *is* is
    /// decided once, in the lexicon, where the write projection and the clause
    /// 4.9 operand parser read it.
    #[test]
    fn a_time_value_is_carried_in_the_spelling_the_client_wrote() {
        for literal in ["2024-01-01T00:00:00.250000Z", "2024-01-01T00:00:00Z", "12:00:00Z", "2024-01-01"] {
            let value: NgsiLdValue = serde_json::from_value(json!(literal)).expect("a string value");
            pretty_assertions::assert_eq!(value, NgsiLdValue::Json(json!(literal)), "{literal}");
            pretty_assertions::assert_eq!(serde_json::to_value(&value).expect("serialize"), json!(literal), "{literal}");
        }
    }

    /// A numeric zone offset is not a clause 4.6.3 `DateTime` at all — "the
    /// trailing timestamp component [...] shall always be equal to the
    /// character 'Z'" — so it is an ordinary string. Sniffing it through a
    /// looser RFC 3339 parse rewrote it to UTC and served a value the client
    /// never wrote.
    #[test]
    fn an_offset_bearing_timestamp_is_not_rewritten_to_utc() {
        let value: NgsiLdValue = serde_json::from_value(json!("2024-01-01T01:00:00+01:00")).expect("a string value");
        pretty_assertions::assert_eq!(serde_json::to_value(&value).expect("serialize"), json!("2024-01-01T01:00:00+01:00"));
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
