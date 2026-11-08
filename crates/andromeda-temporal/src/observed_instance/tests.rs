//! The observation each written instance records, family by family.

use crate::{error::TemporalError, observed_instance::ObservedInstance};
use andromeda_ngsi_ld_entity::{document::instance_document::InstanceDocument, expanded_value_name::ExpandedValueName, geometry_value::GeometryValue};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    identifier::Identifier,
    time::{date::NgsiLdDate, rendering::instant_text, time_of_day::NgsiLdTime},
};
use andromeda_ngsi_ld_model::temporal::{TemporalValue, text_annotation::TextAnnotation};
use andromeda_ngsi_ld_number::{JsonNumber, declared_number::DeclaredNumber};
use andromeda_ngsi_ld_vocabulary::expanded::{classes as expanded_classes, properties as expanded_properties};
use langtag::LangTagBuf;
use serde_json::{Value, json};

const PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/Property";
const RELATIONSHIP: &str = "https://uri.etsi.org/ngsi-ld/Relationship";
const GEO_PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/GeoProperty";
const LIST_RELATIONSHIP: &str = "https://uri.etsi.org/ngsi-ld/ListRelationship";

/// Read the expanded instance `instance` of `speed`.
fn read(instance: &Value) -> Result<Option<ObservedInstance>, TemporalError> {
    let attribute: AttributeName = "https://example.org/speed".parse().expect("a valid attribute name");
    let document = InstanceDocument::from_slice(&attribute, &serde_json::to_vec(instance).expect("JSON")).expect("an expanded instance");
    ObservedInstance::read(document)
}

/// The observation `instance` records.
fn observed(instance: &Value) -> ObservedInstance {
    read(instance).expect("a readable instance").expect("an observation")
}

/// The value `instance` records.
fn recorded(instance: &Value) -> TemporalValue {
    observed(instance).value
}

/// The JSON a history row stores a structured value as.
fn stored(value: &TemporalValue) -> Value {
    let TemporalValue::Structured(structured) = value else {
        panic!("a structured value: {value:?}");
    };
    serde_json::from_str(structured.to_stored_json().expect("the value writes").get()).expect("the stored JSON parses")
}

fn relationship(targets: &[&str]) -> Value {
    let nodes: Vec<Value> = targets.iter().map(|target| json!({"@id": target})).collect();
    json!({"@type": [RELATIONSHIP], expanded_properties::OBJECT.as_str(): nodes})
}

/// A Property instance whose value is the expanded `values`.
fn property(values: &Value) -> Value {
    json!({"@type": [PROPERTY], expanded_properties::VALUE.as_str(): values})
}

/// A Property instance whose value node carries the bare literal `literal`.
fn bare_property(literal: &str) -> Value {
    property(&json!([{"@value": literal}]))
}

/// A Property instance whose value node carries `literal` typed `type_iri`.
fn typed_property(type_iri: &str, literal: &str) -> Value {
    property(&json!([{"@value": literal, "@type": type_iri}]))
}

/// Clause 4.5.3.2: `object` is "a URI or array of URIs", so a single target
/// keeps the URI shape.
#[test]
fn a_single_target_relationship_is_recorded_as_one_uri() {
    pretty_assertions::assert_eq!(
        recorded(&relationship(&["urn:ngsi-ld:Person:Alice"])),
        TemporalValue::Uri("urn:ngsi-ld:Person:Alice".parse().expect("a valid URI"))
    );
}

/// Clause 4.5.3.2 and clause 4.5.9 EXAMPLE 7: every target of a multi-target
/// Relationship survives, in the order the instance declared them.
#[test]
fn every_target_of_a_multi_target_relationship_survives() {
    let value = recorded(&relationship(&[
        "urn:ngsi-ld:Device:14142",
        "urn:ngsi-ld:Device:13562",
        "urn:ngsi-ld:Device:37309",
    ]));
    pretty_assertions::assert_eq!(
        stored(&value),
        json!(["urn:ngsi-ld:Device:14142", "urn:ngsi-ld:Device:13562", "urn:ngsi-ld:Device:37309"])
    );
}

/// A Relationship with no object at all is refused rather than stored with an
/// empty target list.
#[test]
fn a_relationship_without_any_object_is_refused() {
    let error = read(&json!({"@type": [RELATIONSHIP]})).expect_err("no target");
    assert!(error.to_string().contains("object"), "{error}");
}

/// RFC 3986 section 6.2.2.2: a percent-encoded reserved character is not
/// equivalent to the character, so history records the target as written,
/// the spelling current state holds.
#[test]
fn a_percent_encoded_target_is_stored_as_written() {
    pretty_assertions::assert_eq!(
        recorded(&relationship(&["urn:ngsi-ld:Person%3AAlice"])),
        TemporalValue::Uri("urn:ngsi-ld:Person%3AAlice".parse().expect("a valid URI"))
    );
}

#[test]
fn a_percent_encoded_list_target_is_stored_as_written() {
    let instance = json!({
        "@type": [LIST_RELATIONSHIP],
        expanded_properties::OBJECT_LIST.as_str(): [{"@list": [{"@id": "urn:ngsi-ld%3ACity%3AAntwerp"}]}]
    });
    pretty_assertions::assert_eq!(stored(&recorded(&instance)), json!(["urn:ngsi-ld%3ACity%3AAntwerp"]));
}

/// Clauses 4.5.22.2 and 4.5.22.3: the normalized `{"object": …}` member, the
/// concise bare URI and the coerced `@id` node all name one target, kept in
/// order.
#[test]
fn every_conformant_object_list_member_spelling_names_its_target() {
    let instance = json!({
        "@type": [LIST_RELATIONSHIP],
        expanded_properties::OBJECT_LIST.as_str(): [{"@list": [
            {expanded_properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:City:Antwerp"}]},
            {"@value": "urn:ngsi-ld:City:Rotterdam"},
            {"@id": "urn:ngsi-ld:City:Ghent"}
        ]}]
    });
    pretty_assertions::assert_eq!(
        stored(&recorded(&instance)),
        json!(["urn:ngsi-ld:City:Antwerp", "urn:ngsi-ld:City:Rotterdam", "urn:ngsi-ld:City:Ghent"])
    );
}

/// Clause 4.6.3 lists a "`DateTime` string for encoding a timestamp" among the
/// data types a Value may have, and the string is its whole representation.
/// Annex C.6: a string declared as no time value is the string it is, kept
/// byte for byte whatever it resembles.
#[test]
fn an_undeclared_string_is_recorded_as_written_whatever_it_resembles() {
    for literal in [
        "2024-01-01T00:00:00Z",
        "2024-01-01T00:00:00.250000Z",
        "12:00:00.5Z",
        "2024-01-01",
        "2024-01-01T01:00:00+01:00",
        "atrium",
    ] {
        pretty_assertions::assert_eq!(
            recorded(&bare_property(literal)),
            TemporalValue::String {
                text: literal.to_owned(),
                annotation: TextAnnotation::Plain,
            },
            "{literal}"
        );
    }
}

#[test]
fn a_typed_time_is_recorded_as_a_time() {
    for spelling in ["12:00:00.5Z", "12:00:00,5Z"] {
        pretty_assertions::assert_eq!(
            recorded(&typed_property(expanded_classes::TIME.as_str(), spelling)),
            TemporalValue::Time {
                time: "12:00:00.5Z".parse::<NgsiLdTime>().expect("a Time"),
                datatype: Some(expanded_classes::TIME.as_str().parse().expect("a datatype")),
            },
            "{spelling}"
        );
    }
}

#[test]
fn a_typed_date_is_recorded_as_a_date() {
    pretty_assertions::assert_eq!(
        recorded(&typed_property(expanded_classes::DATE.as_str(), "2026-04-12")),
        TemporalValue::Date {
            date: "2026-04-12".parse::<NgsiLdDate>().expect("a Date"),
            datatype: Some(expanded_classes::DATE.as_str().parse().expect("a datatype")),
        }
    );
}

/// Annex C.6: `valueType` declares the datatype of the plain value beside it.
#[test]
fn a_value_type_declares_the_time_value_of_a_plain_literal() {
    let instance = json!({
        "@type": [PROPERTY],
        expanded_properties::VALUE.as_str(): [{"@value": "2024-01-01T00:00:00.250000Z"}],
        expanded_properties::VALUE_TYPE.as_str(): [{"@id": expanded_classes::DATE_TIME.as_str()}],
    });
    let value = recorded(&instance);
    let TemporalValue::DateTime { instant, datatype } = value else {
        panic!("a declared DateTime is an instant, got {value:?}");
    };
    pretty_assertions::assert_eq!(instant_text(instant), "2024-01-01T00:00:00.250Z");
    pretty_assertions::assert_eq!(datatype, None, "the valueType declares the form, the literal does not");
}

/// A declared `@type` refuses a literal that does not satisfy it (annex C.6):
/// a numeric offset on a `Time`, a `Time` without its designator, an unpadded
/// `Date`, a `DateTime` past microsecond resolution or not a timestamp at all.
#[test]
fn a_declared_type_refuses_a_literal_that_does_not_satisfy_it() {
    for (type_iri, literal) in [
        (expanded_classes::TIME.as_str(), "13:00:00+01:00"),
        (expanded_classes::TIME.as_str(), "12:00:00"),
        (expanded_classes::DATE.as_str(), "2026-4-2"),
        (expanded_classes::DATE_TIME.as_str(), "2026-07-25T10:00:00.1234569Z"),
        (expanded_classes::DATE_TIME.as_str(), "yesterday"),
        (expanded_classes::DATE_TIME.as_str(), "2026-07-25T12:00:00+02:00"),
    ] {
        let error = read(&typed_property(type_iri, literal)).expect_err("the literal does not satisfy its declared type");
        assert!(matches!(error, TemporalError::Validation(_)), "{literal}: {error:?}");
        assert!(error.to_string().contains("no value of its declared datatype"), "{literal}: {error}");
    }
}

/// A declared `DateTime` lands in the instant column, not the string column.
#[test]
fn a_datetime_typed_property_lands_in_the_instant_column() {
    assert!(matches!(
        recorded(&typed_property(expanded_classes::DATE_TIME.as_str(), "2026-07-25T10:00:00Z")),
        TemporalValue::DateTime { datatype: Some(_), .. }
    ));
}

/// A structured value is recorded with every typed time value inside it in
/// its served spelling, as current state holds it.
#[test]
fn a_typed_time_inside_a_structured_value_is_recorded_in_its_served_spelling() {
    let at = |text: &str| json!([{"@value": text, "@type": expanded_classes::TIME.as_str()}]);
    let value = recorded(&property(&json!([{"https://example.org/at": at("12:00:00.000Z")}])));
    pretty_assertions::assert_eq!(stored(&value), json!({"https://example.org/at": at("12:00:00Z")}));
}

/// The number keeps the exact shape it was written in all the way to the
/// arbitrary-precision column: one above 2^53, which no double names, and 2^53
/// itself.
#[test]
fn a_number_reaches_its_column_exactly() {
    for (raw, expected) in [
        ("12.5", "12.5"),
        ("9007199254740993", "9007199254740993"),
        ("9007199254740992", "9007199254740992"),
    ] {
        let instance: Value = serde_json::from_str(&format!(
            r#"{{"@type": ["{PROPERTY}"], "{}": [{{"@value": {raw}}}]}}"#,
            expanded_properties::VALUE.as_str()
        ))
        .expect("a well-formed expanded property");
        pretty_assertions::assert_eq!(
            recorded(&instance),
            TemporalValue::Number {
                number: expected.parse::<JsonNumber>().expect("an exact number"),
                datatype: None
            },
            "{raw}"
        );
    }
}

/// Clause 5.2.5 puts no restriction on a Property `value`: an array holding
/// several JSON objects expands to several node objects, each kept as the node
/// it is.
#[test]
fn an_array_of_several_objects_is_kept_node_by_node() {
    let first = json!({"https://example.org/zone": [{"@value": "Europe/Andorra"}]});
    let second = json!({"https://example.org/zone": [{"@value": "Asia/Dubai"}]});
    let value = recorded(&property(&json!([first, second])));
    pretty_assertions::assert_eq!(stored(&value), json!([first, second]));
}

/// A scalar beside an object keeps the bare-scalar shape an all-scalar array is
/// stored in, and the object keeps its node; a JSON literal element is a node
/// too, kept whole.
#[test]
fn a_mixed_array_keeps_each_element_in_its_stored_shape() {
    let node = json!({"https://example.org/a": [{"@value": 1}]});
    pretty_assertions::assert_eq!(stored(&recorded(&property(&json!([{"@value": 1}, node])))), json!([1, node]));
    let literal = json!({"@value": {"k": [1, 2]}, "@type": "@json"});
    pretty_assertions::assert_eq!(stored(&recorded(&property(&json!([{"@value": 3}, literal])))), json!([3, literal]));
}

/// The plain scalars of an array are stored bare, and a scalar with a `@type`
/// keeps its expanded form: clause 4.5.7 records each instance as the Property
/// it was, so the datatype written on an element survives the history.
#[test]
fn an_array_stores_plain_scalars_bare_and_keeps_a_typed_scalar_expanded() {
    let typed = json!({"@value": "2024-01-01T00:00:00Z", "@type": expanded_classes::DATE_TIME.as_str()});
    let value = recorded(&property(&json!([{"@value": "a"}, typed, {"@value": true}])));
    pretty_assertions::assert_eq!(stored(&value), json!(["a", typed, true]));
}

/// The clause 5.6.15 instance-replacement path reads its instance here too, so
/// an array-valued Property replaced through it keeps every element, exactly as
/// one appended through clause 5.6.12 does.
#[test]
fn a_replaced_array_valued_property_keeps_every_element() {
    let value = recorded(&property(&json!([{"@value": 1}, {"@value": 2}, {"@value": 3}])));
    pretty_assertions::assert_eq!(stored(&value), json!([1, 2, 3]));
}

/// What an instance carries beside its value is its metadata, and only that:
/// its identity and time axes have columns of their own, and its clause 4.8
/// system times would echo a current-state time back as a sub-Property of a
/// past observation.
#[test]
fn the_metadata_holds_neither_the_value_nor_the_identity_nor_the_time_axes() {
    let accuracy = json!([{"@type": [PROPERTY], expanded_properties::VALUE.as_str(): [{"@value": 0.5}]}]);
    let instance = json!({
        "@type": [PROPERTY],
        expanded_properties::VALUE.as_str(): [{"@value": 1}],
        expanded_properties::OBSERVED_AT.as_str(): [{"@value": "2024-01-01T00:00:00Z", "@type": expanded_classes::DATE_TIME.as_str()}],
        expanded_properties::CREATED_AT.as_str(): [{"@value": "2024-01-01T00:00:00.000000Z", "@type": expanded_classes::DATE_TIME.as_str()}],
        expanded_properties::MODIFIED_AT.as_str(): [{"@value": "2024-01-02T00:00:00.000000Z", "@type": expanded_classes::DATE_TIME.as_str()}],
        expanded_properties::INSTANCE_ID.as_str(): [{"@id": "urn:ngsi-ld:instance:1"}],
        expanded_properties::DATASET_ID.as_str(): [{"@id": "urn:ngsi-ld:Dataset:a"}],
        expanded_properties::UNIT_CODE.as_str(): [{"@value": "KMH"}],
        "https://example.org/accuracy": accuracy
    });
    let observation = observed(&instance);
    pretty_assertions::assert_eq!(observation.instance_id.as_ref().map(Identifier::as_str), Some("urn:ngsi-ld:instance:1"));
    pretty_assertions::assert_eq!(observation.dataset_id.as_ref().map(Identifier::as_str), Some("urn:ngsi-ld:Dataset:a"));
    assert!(observation.observed_at.is_some());
    let metadata = observation.metadata.expect("the unit and the sub-attribute are metadata");
    pretty_assertions::assert_eq!(
        serde_json::to_value(InstanceDocument::from(metadata)).expect("metadata writes"),
        json!({
            expanded_properties::UNIT_CODE.as_str(): [{"@value": "KMH"}],
            "https://example.org/accuracy": accuracy
        })
    );
}

/// The geometry of a `GeoProperty` is recorded as the geometry it is, whether
/// expansion made a `GeoJSON` node of it or the client wrote the clause 4.7.2
/// string.
#[test]
fn a_geo_property_records_its_geometry_however_it_was_written() {
    let polygon = json!({
        "type": "Polygon",
        "coordinates": [
            [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]],
            [[1.0, 1.0], [1.0, 2.0], [2.0, 2.0], [1.0, 1.0]]
        ]
    });
    let written = Geometry::from_geojson_text(&polygon.to_string()).expect("the fixture is a valid Polygon");
    let point = |x: f64, y: f64| json!({"@list": [{"@value": x}, {"@value": y}]});
    let node = json!({
        "@type": ["https://purl.org/geojson/vocab#Polygon"],
        "https://purl.org/geojson/vocab#coordinates": [{"@list": [
            {"@list": [point(0.0, 0.0), point(4.0, 0.0), point(4.0, 4.0), point(0.0, 4.0), point(0.0, 0.0)]},
            {"@list": [point(1.0, 1.0), point(1.0, 2.0), point(2.0, 2.0), point(1.0, 1.0)]}
        ]}]
    });
    for geometry in [node, json!({"@value": polygon.to_string()})] {
        let instance = json!({"@type": [GEO_PROPERTY], expanded_properties::VALUE.as_str(): [geometry]});
        pretty_assertions::assert_eq!(
            recorded(&instance),
            TemporalValue::Geometry(GeometryValue::new(written.form().clone())),
            "{instance}"
        );
    }
}

/// Clause 4.5.0: an instance whose value is its family's deletion encoding
/// ends an instance rather than observing one, and an instance naming neither
/// a family nor a value observes nothing.
#[test]
fn a_deletion_or_an_empty_instance_records_no_observation() {
    for instance in [
        json!({"@type": [GEO_PROPERTY], expanded_properties::VALUE.as_str(): [{"@value": "urn:ngsi-ld:null"}]}),
        json!({"@type": [RELATIONSHIP], expanded_properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:null"}]}),
        json!({expanded_properties::OBSERVED_AT.as_str(): [{"@value": "2024-01-01T00:00:00Z"}]}),
    ] {
        assert!(read(&instance).expect("readable").is_none(), "{instance}");
    }
}

/// An `observedAt` is held to the clause 4.6.3 strict UTC form like every
/// other client `DateTime`.
#[test]
fn an_observed_at_outside_the_strict_utc_form_is_refused() {
    let mut instance = bare_property("free");
    instance[expanded_properties::OBSERVED_AT.as_str()] = json!([{"@value": "2024-01-01T01:00:00+01:00"}]);
    let attribute: AttributeName = "https://example.org/speed".parse().expect("a valid attribute name");
    let error = InstanceDocument::from_slice(&attribute, &serde_json::to_vec(&instance).expect("JSON")).expect_err("a numeric offset is not the UTC form");
    assert!(error.to_string().contains("observedAt"), "{error}");
}

/// IETF RFC 7946 sections 5 and 6.1, clause 4.6.4: a geometry node records
/// its `bbox` and foreign members with its positions.
#[test]
fn a_recorded_geo_observation_keeps_its_members() {
    let node = json!({
        "@type": ["https://purl.org/geojson/vocab#Point"],
        "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 1.0}, {"@value": 2.0}]}],
        "https://purl.org/geojson/vocab#bbox": [{"@list": [{"@value": 1.0}, {"@value": 2.0}, {"@value": 1.0}, {"@value": 2.0}]}],
        "https://ex.org/plotName": [{"@value": "plot 7"}]
    });
    let instance = json!({"@type": [GEO_PROPERTY], expanded_properties::VALUE.as_str(): [node]});
    let TemporalValue::Geometry(recorded) = recorded(&instance) else {
        panic!("a geometry observation");
    };
    let members = recorded.members().expect("the members are recorded");
    pretty_assertions::assert_eq!(members.bbox().map(|bbox| bbox.ordinates().collect::<Vec<_>>()), Some(vec![1.0, 2.0, 1.0, 2.0]));
    pretty_assertions::assert_eq!(
        members.foreign().keys().map(ExpandedValueName::as_str).collect::<Vec<_>>(),
        vec!["https://ex.org/plotName"]
    );
}

/// Clause 3.1: the empty array is a Property value, recorded as the empty
/// array rather than refused as an instance carrying none.
#[test]
fn an_empty_array_valued_property_is_recorded_as_the_empty_array() {
    let value = recorded(&property(&json!([])));
    pretty_assertions::assert_eq!(stored(&value), json!([]));
}

/// The metadata `instance` records, as the expanded members it renders as.
fn metadata_of(instance: &Value) -> Value {
    serde_json::to_value(InstanceDocument::from(observed(instance).metadata.expect("metadata"))).expect("metadata writes")
}

/// Table 5.2.5-2: `createdAt`, `modifiedAt`, `deletedAt` and `instanceId`
/// are output-only, and a broker ignores them when a producer writes them, on
/// the instance and on its sub-attributes at every depth.
#[test]
fn output_only_members_a_producer_wrote_are_not_metadata() {
    let date_time = |text: &str| json!([{"@value": text, "@type": expanded_classes::DATE_TIME.as_str()}]);
    let method = json!([{
        "@type": [PROPERTY],
        expanded_properties::VALUE.as_str(): [{"@value": "gps"}],
        expanded_properties::INSTANCE_ID.as_str(): [{"@id": "urn:ngsi-ld:instance:2"}],
        expanded_properties::DELETED_AT.as_str(): date_time("2024-01-03T00:00:00Z")
    }]);
    let instance = json!({
        "@type": [PROPERTY],
        expanded_properties::VALUE.as_str(): [{"@value": 1}],
        expanded_properties::DELETED_AT.as_str(): date_time("2024-01-02T00:00:00Z"),
        "https://example.org/accuracy": [{
            "@type": [PROPERTY],
            expanded_properties::VALUE.as_str(): [{"@value": 0.5}],
            expanded_properties::INSTANCE_ID.as_str(): [{"@id": "urn:ngsi-ld:instance:1"}],
            expanded_properties::DELETED_AT.as_str(): date_time("2024-01-02T00:00:00Z"),
            expanded_properties::CREATED_AT.as_str(): date_time("2024-01-01T00:00:00Z"),
            "https://example.org/method": method
        }]
    });
    pretty_assertions::assert_eq!(
        metadata_of(&instance),
        json!({
            "https://example.org/accuracy": [{
                "@type": [PROPERTY],
                expanded_properties::VALUE.as_str(): [{"@value": 0.5}],
                "https://example.org/method": [{"@type": [PROPERTY], expanded_properties::VALUE.as_str(): [{"@value": "gps"}]}]
            }]
        })
    );
    let deleted_only = json!({
        "@type": [PROPERTY],
        expanded_properties::VALUE.as_str(): [{"@value": 1}],
        expanded_properties::DELETED_AT.as_str(): date_time("2024-01-02T00:00:00Z")
    });
    assert!(observed(&deleted_only).metadata.is_none());
}

/// Clause 5.5.4 admits the NGSI-LD Null in the temporal evolution only as the
/// value of a deleted instance (clause 4.5.7); a sub-attribute of a recorded
/// instance is a Property as clause 4.5.2 mandates.
#[test]
fn a_sub_attribute_holding_the_null_is_refused() {
    let instance = json!({
        "@type": [PROPERTY],
        expanded_properties::VALUE.as_str(): [{"@value": 1}],
        "https://example.org/accuracy": [{"@type": [PROPERTY], expanded_properties::VALUE.as_str(): [{"@value": "urn:ngsi-ld:null"}]}]
    });
    assert!(matches!(read(&instance), Err(TemporalError::Validation(_))), "{:?}", read(&instance));
}

/// A sub-attribute's instants, its `DateTime` value and its list targets are
/// recorded in their served spelling, as the instance's own `observedAt` is.
#[test]
fn a_sub_attribute_is_recorded_in_its_served_spelling() {
    let date_time = |text: &str| json!([{"@value": text, "@type": expanded_classes::DATE_TIME.as_str()}]);
    let instance = json!({
        "@type": [PROPERTY],
        expanded_properties::VALUE.as_str(): [{"@value": 1}],
        "https://example.org/accuracy": [{
            "@type": [PROPERTY],
            expanded_properties::VALUE.as_str(): date_time("2026-01-01T00:00:00.5Z"),
            expanded_properties::OBSERVED_AT.as_str(): date_time("2026-01-01T00:00:00.25Z")
        }],
        "https://example.org/sources": [{
            "@type": [LIST_RELATIONSHIP],
            expanded_properties::OBJECT_LIST.as_str(): [{"@list": [{"@id": "urn:ngsi-ld:Source:1"}]}]
        }]
    });
    let metadata = metadata_of(&instance);
    pretty_assertions::assert_eq!(
        metadata["https://example.org/accuracy"][0][expanded_properties::VALUE.as_str()],
        date_time("2026-01-01T00:00:00.500Z")
    );
    pretty_assertions::assert_eq!(
        metadata["https://example.org/accuracy"][0][expanded_properties::OBSERVED_AT.as_str()],
        date_time("2026-01-01T00:00:00.250Z")
    );
    pretty_assertions::assert_eq!(
        metadata["https://example.org/sources"][0][expanded_properties::OBJECT_LIST.as_str()],
        json!([{"@list": [{expanded_properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:Source:1"}]}]}])
    );
}

/// Clause 5.5.4 admits the NGSI-LD Null in a temporal evolution in the value
/// position of a deleted instance alone; as a companion member of a recorded
/// instance it is refused.
#[test]
fn a_companion_member_holding_the_null_is_refused() {
    let instance = json!({
        "@type": [PROPERTY],
        expanded_properties::VALUE.as_str(): [{"@value": 1}],
        expanded_properties::UNIT_CODE.as_str(): [{"@value": "urn:ngsi-ld:null"}]
    });
    assert!(matches!(read(&instance), Err(TemporalError::Validation(_))), "{:?}", read(&instance));
}

/// Clause 4.5.2 admits a JSON-LD value object as a Property value: a
/// language-tagged string keeps its `@language`.
#[test]
fn a_language_tagged_string_is_recorded_with_its_language() {
    let value = recorded(&property(&json!([{"@value": "hola", "@language": "es"}])));
    pretty_assertions::assert_eq!(
        value,
        TemporalValue::String {
            text: "hola".to_owned(),
            annotation: TextAnnotation::Language(LangTagBuf::new("es".to_owned()).expect("a language tag")),
        }
    );
}

/// A typed string and a typed number keep the datatype they declare; a string
/// declared as an XSD number or `xsd:boolean` is that number or Boolean.
#[test]
fn a_typed_literal_is_recorded_with_its_datatype() {
    const INTEGER: &str = "http://www.w3.org/2001/XMLSchema#integer";
    const DOUBLE: &str = "http://www.w3.org/2001/XMLSchema#double";
    pretty_assertions::assert_eq!(
        recorded(&property(&json!([{"@value": "42", "@type": INTEGER}]))),
        TemporalValue::DeclaredNumber {
            number: DeclaredNumber::Finite(JsonNumber::from(42)),
            datatype: Some(INTEGER.parse().expect("a datatype")),
        }
    );
    pretty_assertions::assert_eq!(
        recorded(&property(&json!([{"@value": "1", "@type": "http://www.w3.org/2001/XMLSchema#boolean"}]))),
        TemporalValue::DeclaredBoolean {
            flag: true,
            datatype: Some("http://www.w3.org/2001/XMLSchema#boolean".parse().expect("a datatype")),
        }
    );
    pretty_assertions::assert_eq!(
        recorded(&property(&json!([{"@value": "42", "@type": "http://www.w3.org/2001/XMLSchema#token"}]))),
        TemporalValue::String {
            text: "42".to_owned(),
            annotation: TextAnnotation::Datatype("http://www.w3.org/2001/XMLSchema#token".parse().expect("a datatype")),
        }
    );
    pretty_assertions::assert_eq!(
        recorded(&property(&json!([{"@value": 7, "@type": DOUBLE}]))),
        TemporalValue::Number {
            number: JsonNumber::from(7),
            datatype: Some(DOUBLE.parse().expect("a datatype")),
        }
    );
}

/// A string with a base direction is kept whole in the structured column, as
/// current state keeps it.
#[test]
fn a_string_with_a_direction_is_recorded_structured() {
    let value = recorded(&property(&json!([{"@value": "hola", "@language": "es", "@direction": "ltr"}])));
    assert!(matches!(value, TemporalValue::Structured(_)), "{value:?}");
}
