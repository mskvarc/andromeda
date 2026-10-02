//! The stored JSON of structured values and metadata, and the durable wire of a
//! history row, pinned against the shapes rows already hold.

use crate::temporal::{
    TemporalHistoryRow,
    TemporalValue,
    observation_metadata::ObservationMetadata,
    structured_value::StructuredValue,
    structured_value_codec::StoredValueError,
};
use andromeda_ngsi_ld_entity::{document::slot::Slot, expanded_value::ExpandedValue, literal::LiteralValue, unit_code::UnitCode};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use andromeda_ngsi_ld_number::JsonNumber;
use serde_json::{Value, json};
use std::str::FromStr;

fn attribute() -> AttributeName {
    "https://example.org/speed".parse().expect("a valid attribute name")
}

/// Decode `stored` as a value of `family` and write it back.
fn round_trip(family: AttributeType, stored: &Value) -> Value {
    let value = StructuredValue::from_stored(family, &attribute(), &stored.to_string()).expect("the stored JSON reads as a value of its family");
    let written = value.to_stored_json().expect("the value writes");
    serde_json::from_str(written.get()).expect("the written JSON parses")
}

/// Every stored shape the history and the current-attribute projection hold
/// reads back and writes out byte-equivalent (key order aside, which `jsonb`
/// does not keep): the aggregation, `distinct`, `aggrGroupBy` and the temporal
/// `q` read inside it.
#[test]
fn every_stored_shape_reads_and_writes_back_unchanged() {
    let cases = [
        (AttributeType::Property, json!(["a", "b"])),
        (AttributeType::Property, json!([1, 2.5, true])),
        (
            AttributeType::Property,
            serde_json::from_str::<Value>("[0.100000000000000005, 9007199254740993]").expect("exact numbers"),
        ),
        (AttributeType::Property, json!([1, {"https://example.org/a": [{"@value": 1}]}])),
        (AttributeType::Property, json!([3, {"@value": {"k": [1, 2]}, "@type": "@json"}])),
        (
            AttributeType::Property,
            json!({"https://uri.etsi.org/ngsi-ld/default-context/unit": [{"@value": "km"}]}),
        ),
        (AttributeType::Property, json!({"@list": [{"@value": 1}, {"@value": 2}]})),
        (AttributeType::Property, json!({"@value": {"k": 1}, "@type": "@json"})),
        (AttributeType::GeoProperty, json!(["a", "b"])),
        (AttributeType::Relationship, json!(["urn:ngsi-ld:Device:1", "urn:ngsi-ld:Device:2"])),
        (AttributeType::VocabProperty, json!(["https://example.org/a", "https://example.org/b"])),
        (
            AttributeType::LanguageProperty,
            json!([{"@language": "en", "@value": "red"}, {"@language": "de", "@value": "rot"}, {"@value": "rouge"}]),
        ),
        (AttributeType::JsonProperty, json!({"k": [1, {"x": null}], "@context": "data"})),
        (AttributeType::JsonProperty, json!([1, "two"])),
        (
            AttributeType::ListProperty,
            json!([{"@value": 1}, {"@value": "a"}, {"@list": [{"@value": true}]}]),
        ),
        (AttributeType::ListProperty, json!([])),
        (AttributeType::ListRelationship, json!(["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"])),
    ];
    for (family, stored) in cases {
        pretty_assertions::assert_eq!(round_trip(family, &stored), stored, "{family}");
    }
}

/// An array-valued Property keeps every element and the order it was written
/// in (clause 4.9 makes the whole array the target value).
#[test]
fn an_array_valued_property_keeps_every_element_in_order() {
    let value = StructuredValue::from_stored(AttributeType::Property, &attribute(), r#"["a", 2, false]"#).expect("a stored array");
    let StructuredValue::Values(values) = value else {
        panic!("a Property reads as its values: {value:?}");
    };
    let scalars = values
        .iter()
        .map(|value| match value {
            ExpandedValue::Literal(literal) => literal.value().clone(),
            other @ (ExpandedValue::Json(_) | ExpandedValue::Node(_) | ExpandedValue::List(_)) => panic!("a scalar element reads as a literal: {other:?}"),
        })
        .collect::<Vec<_>>();
    pretty_assertions::assert_eq!(
        scalars,
        vec![
            LiteralValue::String("a".into()),
            LiteralValue::Number(JsonNumber::from(2)),
            LiteralValue::Boolean(false)
        ]
    );
}

/// The same array of strings is a different value in each family that stores
/// one, which is why reading needs the family.
#[test]
fn an_array_of_strings_reads_as_the_value_of_its_family() {
    let stored = r#"["urn:ngsi-ld:A", "urn:ngsi-ld:B"]"#;
    assert!(matches!(
        StructuredValue::from_stored(AttributeType::Relationship, &attribute(), stored),
        Ok(StructuredValue::Objects(_))
    ));
    assert!(matches!(
        StructuredValue::from_stored(AttributeType::ListRelationship, &attribute(), stored),
        Ok(StructuredValue::ObjectList(_))
    ));
    assert!(matches!(
        StructuredValue::from_stored(AttributeType::Property, &attribute(), stored),
        Ok(StructuredValue::Values(_))
    ));
}

/// A stored target that is no URI is a broken row, not a value.
#[test]
fn a_target_that_is_no_uri_is_refused() {
    assert!(matches!(
        StructuredValue::from_stored(AttributeType::Relationship, &attribute(), r#"["not a uri"]"#),
        Err(StoredValueError::Target(_))
    ));
    assert!(matches!(
        StructuredValue::from_stored(AttributeType::Relationship, &attribute(), "[]"),
        Err(StoredValueError::Empty(_))
    ));
}

/// The metadata object reads back and writes out unchanged, nested
/// sub-attributes and companion members included.
#[test]
fn metadata_reads_and_writes_back_unchanged() {
    let stored = json!({
        "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "CEL"}],
        "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@value": "2030-01-01T00:00:00Z", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}],
        "https://example.org/accuracy": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 0.5}]
        }]
    });
    let metadata = ObservationMetadata::from_stored(AttributeType::Property, &attribute(), &stored.to_string())
        .expect("stored metadata reads")
        .expect("the members are metadata");
    let written: Value = serde_json::from_str(metadata.to_stored_json().expect("metadata writes").get()).expect("the written JSON parses");
    pretty_assertions::assert_eq!(written, stored);
}

/// The members a history row keeps in columns of their own never stay in its
/// metadata, whatever an older row stored there.
#[test]
fn column_members_are_not_metadata() {
    let stored = json!({
        "https://uri.etsi.org/ngsi-ld/createdAt": [{"@value": "2024-01-01T00:00:00.000000Z", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}],
        "https://uri.etsi.org/ngsi-ld/observedAt": [{"@value": "2024-01-01T00:00:00Z", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}]
    });
    pretty_assertions::assert_eq!(
        ObservationMetadata::from_stored(AttributeType::Property, &attribute(), &stored.to_string()).expect("readable"),
        None
    );
}

/// The unit leaves the metadata for its column and comes back exactly.
#[test]
fn the_unit_code_round_trips_through_its_column() {
    let stored = json!({
        "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "KGM"}],
        "https://example.org/accuracy": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}]
    });
    let metadata = ObservationMetadata::from_stored(AttributeType::Property, &attribute(), &stored.to_string())
        .expect("readable")
        .expect("metadata");
    let (rest, unit_code) = metadata.take_unit_code();
    let Some(Slot::Set(unit_code)) = unit_code else {
        panic!("the unit is taken: {unit_code:?}");
    };
    pretty_assertions::assert_eq!(unit_code, UnitCode::resolve("KGM").expect("a common code"));
    let rest_json: Value = serde_json::from_str(rest.as_ref().expect("the sub-attribute remains").to_stored_json().expect("writes").get()).expect("parses");
    assert!(rest_json.get("https://uri.etsi.org/ngsi-ld/unitCode").is_none(), "{rest_json}");
    let restored = ObservationMetadata::with_unit_code(rest, unit_code);
    let restored_json: Value = serde_json::from_str(restored.to_stored_json().expect("writes").get()).expect("parses");
    pretty_assertions::assert_eq!(restored_json, stored);

    let unit_only = ObservationMetadata::from_stored(
        AttributeType::Property,
        &attribute(),
        r#"{"https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "MTR"}]}"#,
    )
    .expect("readable")
    .expect("metadata");
    pretty_assertions::assert_eq!(unit_only.take_unit_code().0, None, "metadata holding only the unit stores no document");
}

/// A durable row of the fixed fields and the given value and metadata.
fn durable_row(value: &Value, metadata: &Value) -> Value {
    json!({
        "attribute_name": "https://example.org/speed",
        "attribute_type": "Relationship",
        "dataset_id": null,
        "value": value,
        "observed_at": null,
        "recorded_at": "2024-01-01T00:00:00Z",
        "created_at": "2024-01-01T00:00:00Z",
        "modified_at": "2024-01-01T00:00:00Z",
        "instance": {"generated": "01942f6c-7f00-7000-8000-000000000001"},
        "metadata": metadata,
        "partition_total": null
    })
}

/// The structured value and the metadata travel in an intent in the JSON a
/// stored row holds, under the `object` tag an untyped value had, so a durable
/// row reads back typed and writes out as it was read.
#[test]
fn a_durable_row_reads_back_typed_and_writes_out_unchanged() {
    let durable = durable_row(
        &json!({"object": ["urn:ngsi-ld:Device:1", "urn:ngsi-ld:Device:2"]}),
        &json!({"https://uri.etsi.org/ngsi-ld/hasObjectType": [{"@id": "https://example.org/Device"}]}),
    );
    let row: TemporalHistoryRow = serde_json::from_value(durable.clone()).expect("a durable row reads"); // clone: the fixture is compared after the read
    assert!(matches!(row.value, TemporalValue::Structured(StructuredValue::Objects(_))), "{row:?}");
    assert!(row.metadata.is_some());
    pretty_assertions::assert_eq!(serde_json::to_value(&row).expect("the row writes"), durable);
    let reread: TemporalHistoryRow = serde_json::from_str(&serde_json::to_string(&row).expect("the row writes")).expect("the written row reads");
    pretty_assertions::assert_eq!(reread, row);
}

/// The scalar values keep the wire shape durable intents have always used.
#[test]
fn scalar_values_keep_their_durable_wire_shape() {
    let number = TemporalValue::Number(JsonNumber::from_str("0.100000000000000005").expect("supported exact number"));
    pretty_assertions::assert_eq!(serde_json::to_string(&number).expect("durable JSON"), r#"{"number":0.100000000000000005}"#);
    let point = Geometry::from_geojson_value(&json!({"type": "Point", "coordinates": [1.0, 2.0]})).expect("a point");
    pretty_assertions::assert_eq!(
        serde_json::to_value(TemporalValue::Geometry(point)).expect("durable JSON"),
        json!({"geometry": {"type": "Point", "coordinates": [1.0, 2.0]}})
    );
    for (wire, expected) in [
        (json!({"number": 42}), TemporalValue::Number(JsonNumber::from(42))),
        (
            json!({"number": 1.5}),
            TemporalValue::Number(JsonNumber::from_str("1.5").expect("exact decimal")),
        ),
        (json!({"string": "free"}), TemporalValue::String("free".to_owned())),
    ] {
        let row: TemporalHistoryRow = serde_json::from_value(durable_row(&wire, &Value::Null)).expect("an old intent row reads");
        pretty_assertions::assert_eq!(row.value, expected);
    }
}

/// A single JSON literal is stored as its value object, so it reads back as
/// the literal rather than as a node its keys would not make.
#[test]
fn a_single_json_literal_reads_back_as_the_literal() {
    let value = StructuredValue::from_stored(AttributeType::Property, &attribute(), r#"{"@value": {"k": 1}, "@type": "@json"}"#).expect("a stored literal");
    let StructuredValue::Values(values) = value else {
        panic!("a Property reads as its values: {value:?}");
    };
    assert!(matches!(values.head(), ExpandedValue::Json(_)), "{values:?}");
}
