use crate::attribute::value::{NgsiLdValue, ValueRefusal};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_geometry::{dimension::GeometryDimension, geojson::GeometryError, position::PositionError};
use serde_json::{Value, json, value::RawValue};

/// The value `json` reads as.
fn read(json: &Value) -> Result<NgsiLdValue, ValueRefusal> {
    match NgsiLdValue::from_node(json) {
        Ok(read) => read,
        Err(never) => match never {},
    }
}

/// Clause 4.6.4: "implementations shall preserve the representation of the
/// content of the values provided by the context information providers and
/// return the original content when replying to context consumption
/// requests." A Property value that happens to parse as a URI or a time is
/// still a value, served back in the spelling the client wrote.
#[test]
fn a_string_value_is_served_back_in_the_spelling_the_client_wrote() {
    for literal in [
        "URN:NGSI-LD:Building:001",
        "HTTPS://Example.COM:443/Path",
        "mailto:someone@example.org",
        "2024-01-01T00:00:00.250000Z",
        "2024-01-01T01:00:00+01:00",
        "12:00:00Z",
    ] {
        let value = read(&json!(literal)).expect("a string value");
        let expected = JsonLiteral::admit(&json!(literal).to_string()).expect("a literal");
        pretty_assertions::assert_eq!(value, NgsiLdValue::Json(expected), "{literal}");
        pretty_assertions::assert_eq!(serde_json::to_value(&value).expect("serialize"), json!(literal), "{literal}");
    }
}

#[test]
fn a_geojson_object_decodes_as_a_geometry_with_its_altitude() {
    let NgsiLdValue::Geometry(flat) = read(&json!({"type": "Point", "coordinates": [13.38, 52.52]})).expect("a point") else {
        panic!("a geometry");
    };
    pretty_assertions::assert_eq!(flat.dimension(), GeometryDimension::TwoD);
    let NgsiLdValue::Geometry(elevated) = read(&json!({"type": "Point", "coordinates": [13.38, 52.52, 34.5]})).expect("a point") else {
        panic!("a geometry");
    };
    pretty_assertions::assert_eq!(elevated.dimension(), GeometryDimension::ThreeD);
}

/// RFC 7946 section 4: a `GeoJSON`-shaped value is a geometry or an error,
/// never an ordinary Property carrying a position that names no point.
#[test]
fn a_geojson_shaped_value_that_is_no_geometry_is_refused_rather_than_demoted() {
    let error = read(&json!({"type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0]})).expect_err("four ordinates have no meaning");
    assert!(
        matches!(
            error,
            ValueRefusal::Geometry(GeometryError::Position {
                source: PositionError::TooManyOrdinates { ordinates: 4 },
                ..
            })
        ),
        "got {error:?}"
    );
    let error = read(&json!({"type": "Point", "coordinates": [0.0, 91.0]})).expect_err("a position off the axes");
    assert!(error.to_string().contains("latitude 91 is outside [-90, 90]"), "got {error}");
}

#[test]
fn a_value_that_is_not_geojson_shaped_is_json() {
    for value in [json!({"type": "Point"}), json!("a string"), json!({"speed": 42}), json!([1, 2])] {
        assert!(matches!(read(&value), Ok(NgsiLdValue::Json(_))), "{value}");
    }
}

/// A value is admitted as a literal: a name written twice in one object is
/// refused.
#[test]
fn a_value_naming_a_member_twice_is_refused() {
    let text = RawValue::from_string(r#"{"a": 1, "a": 2}"#.to_owned()).expect("JSON text");
    let read = NgsiLdValue::from_node(andromeda_json_literal::piece::JsonPiece::new(&text));
    assert!(matches!(read, Ok(Err(ValueRefusal::Literal(_)))), "{read:?}");
}
