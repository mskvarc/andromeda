use crate::attribute::value::NgsiLdValue;
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError, piece::JsonPiece};
use serde_json::{Value, json, value::RawValue};

/// The value `json` reads as, from the text it is written as.
fn read(json: &Value) -> Result<NgsiLdValue, JsonLiteralError> {
    read_text(&json.to_string())
}

/// The value the JSON text `text` reads as.
fn read_text(text: &str) -> Result<NgsiLdValue, JsonLiteralError> {
    let text = RawValue::from_string(text.to_owned()).expect("the fixture is JSON");
    match NgsiLdValue::from_node(JsonPiece::new(&text)) {
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
        pretty_assertions::assert_eq!(value, NgsiLdValue::from(expected), "{literal}");
        pretty_assertions::assert_eq!(serde_json::to_value(&value).expect("serialize"), json!(literal), "{literal}");
    }
}

/// Clause 5.2.5 admits any JSON value for a Property, and the clause 4.7.1
/// geometry rules bind a `GeoProperty` only: a `GeoJSON`-shaped value is the
/// JSON it was written as, valid geometry or not.
#[test]
fn a_geojson_shaped_value_is_json() {
    for value in [
        r#"{"type":"Point","coordinates":[1,2,3,4]}"#,
        r#"{"type":"Point","coordinates":[0,91]}"#,
        r#"{"type":"GeometryCollection","geometries":[{"type":"Point","coordinates":[13,52]}]}"#,
        r#"{"type":"LineString","coordinates":[[13,52],[13,52,34.5]]}"#,
    ] {
        let read = read_text(value).expect("any JSON value");
        pretty_assertions::assert_eq!(read, NgsiLdValue::from(JsonLiteral::admit(value).expect("a literal")), "{value}");
        pretty_assertions::assert_eq!(serde_json::to_string(&read).expect("serialize"), value, "{value}");
    }
}

/// A Property value keeps every significant digit (clause 4.6.4), an ordinate
/// of `GeoJSON` it holds included: no geometry narrows it to a double.
#[test]
fn an_ordinate_in_a_property_value_keeps_every_digit() {
    let value = r#"{"type":"Point","coordinates":[13.123456789012345678,52.5]}"#;
    let read = read_text(value).expect("any JSON value");
    pretty_assertions::assert_eq!(serde_json::to_string(&read).expect("serialize"), value);
}

/// A value is admitted as a literal: a name written twice in one object is
/// refused.
#[test]
fn a_value_naming_a_member_twice_is_refused() {
    let text = RawValue::from_string(r#"{"a": 1, "a": 2}"#.to_owned()).expect("JSON text");
    let read = NgsiLdValue::from_node(JsonPiece::new(&text));
    assert!(matches!(read, Ok(Err(_))), "{read:?}");
}
