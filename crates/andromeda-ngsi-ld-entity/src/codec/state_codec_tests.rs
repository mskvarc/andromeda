//! Round trips and refusals of the stored-document codec.

use crate::{
    codec::stored_document_fixtures::{
        COORDINATES,
        GEO_PROPERTY,
        GEOJSON,
        LIST_RELATIONSHIP,
        PROPERTY,
        RELATIONSHIP,
        VOCAB_PROPERTY,
        date_time,
        entity,
        every_family_document,
        ngsi,
        property,
        stamped,
        stored_order_document,
    },
    entity_state::EntityState,
    family_value::FamilyValue,
    geo_value::GeoValue,
    list_object::ListObject,
    member_shape::MemberShape,
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_geometry::{bounding_box::BoundingBoxError, form::GeometryForm, geojson::GeometryError};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_number::JsonNumber;
use serde_json::{Value, json};

/// `value` with every number replaced by its canonical exact spelling, so two
/// documents compare by numeric value as the round-trip contract states.
fn by_value(value: Value) -> Value {
    match value {
        Value::Number(number) => JsonNumber::try_from(&number).map_or(Value::Number(number), |exact| exact.to_json_value()),
        Value::Array(items) => Value::Array(items.into_iter().map(by_value).collect()),
        Value::Object(members) => Value::Object(members.into_iter().map(|(key, member)| (key, by_value(member))).collect()),
        other @ (Value::Null | Value::Bool(_) | Value::String(_)) => other,
    }
}

/// Decode `document`, encode the state, and parse the result back.
fn round_trip(document: &Value) -> Value {
    let bytes = serde_json::to_vec(document).expect("a JSON document");
    let state = EntityState::from_slice(&bytes).expect("a stored entity");
    let encoded = state.to_vec().expect("the state encodes");
    serde_json::from_slice(&encoded).expect("the encoding is JSON")
}

/// Decode `document` and return the refusal.
fn refusal(document: &Value) -> EntityRefusal {
    let bytes = serde_json::to_vec(document).expect("a JSON document");
    EntityState::from_slice(&bytes).expect_err("the document is refused")
}

#[test]
fn every_family_round_trips_with_every_optional_member() {
    let document = every_family_document();
    pretty_assertions::assert_eq!(round_trip(&document), document);
}

#[test]
fn instances_keep_their_stored_order_with_the_default_in_the_middle() {
    let document = stored_order_document();
    pretty_assertions::assert_eq!(round_trip(&document), document);
}

#[test]
fn list_relationship_items_keep_their_bare_or_wrapped_spelling() {
    let document = entity(&json!({
        "https://ex.org/route": [stamped(json!({
            "@type": [LIST_RELATIONSHIP],
            ngsi("hasObjectList"): [{"@list": [
                {ngsi("hasObject"): [{"@id": "urn:ngsi-ld:Stop:2"}]},
                {"@id": "urn:ngsi-ld:Stop:1"}
            ]}]
        }))]
    }));
    let bytes = serde_json::to_vec(&document).expect("JSON");
    let state = EntityState::from_slice(&bytes).expect("a stored entity");
    let objects = state
        .attributes()
        .get("https://ex.org/route")
        .and_then(|instances| instances.get(None))
        .map(|instance| instance.value().clone());
    let Some(FamilyValue::ListRelationship { objects, .. }) = objects else {
        panic!("a ListRelationship");
    };
    assert!(matches!(objects.as_slice(), [ListObject::Wrapped(_), ListObject::Bare(_)]));
    pretty_assertions::assert_eq!(round_trip(&document), document);
}

#[test]
fn list_relationship_items_written_as_plain_strings_round_trip_exactly() {
    let document = entity(&json!({
        "https://ex.org/route": [stamped(json!({
            "@type": [LIST_RELATIONSHIP],
            ngsi("hasObjectList"): [{"@list": [
                {"@value": "urn:ngsi-ld:Stop:1"},
                {"@id": "urn:ngsi-ld:Stop:2"}
            ]}]
        }))]
    }));
    let bytes = serde_json::to_vec(&document).expect("JSON");
    let state = EntityState::from_slice(&bytes).expect("a stored entity");
    let objects = state
        .attributes()
        .get("https://ex.org/route")
        .and_then(|instances| instances.get(None))
        .map(|instance| instance.value().clone());
    let Some(FamilyValue::ListRelationship { objects, .. }) = objects else {
        panic!("a ListRelationship");
    };
    assert!(matches!(objects.as_slice(), [ListObject::Literal(_), ListObject::Bare(_)]));
    pretty_assertions::assert_eq!(round_trip(&document), document);
}

#[test]
fn numbers_are_written_in_their_canonical_exact_spelling() {
    let document = entity(&json!({
        "https://ex.org/n": [property(&serde_json::from_str::<Value>(
            r#"[{"@value": 1.50}, {"@value": -0}, {"@value": 1e2}, {"@value": 123456789012345678901234567890}, {"@value": 0.1}, {"@value": 1e-400}, {"@value": 12345678901234567890.123456789}]"#
        ).expect("numbers"))]
    }));
    let encoded = round_trip(&document);
    let values: Vec<String> = encoded[0]["https://ex.org/n"][0][ngsi("hasValue")]
        .as_array()
        .into_iter()
        .flatten()
        .map(|node| node["@value"].to_string())
        .collect();
    pretty_assertions::assert_eq!(
        values,
        vec![
            "1.5",
            "0",
            "100",
            "123456789012345678901234567890",
            "0.1",
            "1e-400",
            "12345678901234567890.123456789"
        ]
    );
}

#[test]
fn every_geometry_type_round_trips_including_altitude_and_mixed_arity() {
    let point = |coordinates: Value| json!({"@list": coordinates});
    let position = |ordinates: &[f64]| point(Value::Array(ordinates.iter().map(|ordinate| json!({"@value": ordinate})).collect()));
    let ring = json!({"@list": [position(&[0.0, 0.0]), position(&[1.0, 0.0]), position(&[1.0, 1.0, 5.0]), position(&[0.0, 0.0])]});
    let geometries = [
        ("Point", position(&[1.5, 2.25])),
        ("MultiPoint", json!({"@list": [position(&[1.0, 2.0]), position(&[3.0, 4.0, 5.0])]})),
        ("LineString", json!({"@list": [position(&[0.0, 0.0]), position(&[1.0, 1.0, 50.0])]})),
        ("MultiLineString", json!({"@list": [{"@list": [position(&[0.0, 0.0]), position(&[1.0, 1.0])]}]})),
        ("Polygon", json!({"@list": [&ring]})),
        ("MultiPolygon", json!({"@list": [{"@list": [&ring]}, {"@list": []}]})),
    ];
    for (geometry_type, coordinates) in geometries {
        let document = entity(&json!({
            ngsi("location"): [stamped(json!({
                "@type": [GEO_PROPERTY],
                ngsi("hasValue"): [{"@type": [format!("{GEOJSON}{geometry_type}")], format!("{GEOJSON}coordinates"): [coordinates]}]
            }))]
        }));
        pretty_assertions::assert_eq!(by_value(round_trip(&document)), by_value(document), "{geometry_type}");
    }
}

#[test]
fn a_string_encoded_geometry_is_kept_as_its_string() {
    let text = "{\"type\":\"Point\",\"coordinates\":[1,2]}";
    let document = entity(&json!({
        ngsi("location"): [stamped(json!({"@type": [GEO_PROPERTY], ngsi("hasValue"): [{"@value": text}]}))]
    }));
    let bytes = serde_json::to_vec(&document).expect("JSON");
    let state = EntityState::from_slice(&bytes).expect("a stored entity");
    let geometry = state
        .attributes()
        .get(&ngsi("location"))
        .and_then(|instances| instances.get(None))
        .and_then(|instance| instance.value().geometry().cloned());
    assert!(matches!(&geometry, Some(GeoValue::Encoded(encoded)) if encoded.as_str() == text && matches!(encoded.form(), GeometryForm::Point(_))));
    pretty_assertions::assert_eq!(round_trip(&document), document);
}

#[test]
fn a_geometry_coming_before_its_type_is_still_read_as_a_geometry() {
    let bytes = format!(
        r#"[{{"@id":"urn:ngsi-ld:B:1","@type":["https://ex.org/B"],"{loc}":[{{"{value}":[{{"{coordinates}":[{{"@list":[{{"@value":1}},{{"@value":2}}]}}],"@type":["{GEOJSON}Point"]}}],"@type":["{GEO_PROPERTY}"]}}]}}]"#,
        loc = ngsi("location"),
        value = ngsi("hasValue"),
        coordinates = COORDINATES,
    );
    let state = EntityState::from_slice(bytes.as_bytes()).expect("a stored entity");
    let geometry = state
        .attributes()
        .get(&ngsi("location"))
        .and_then(|instances| instances.get(None))
        .and_then(|instance| instance.value().geometry().map(GeoValue::form).cloned());
    assert!(matches!(geometry, Some(GeometryForm::Point(_))));
}

#[test]
fn two_instances_with_one_dataset_id_are_refused_naming_the_attribute_and_the_dataset() {
    let instance = json!({"@type": [PROPERTY], ngsi("hasValue"): [{"@value": 1}], ngsi("datasetId"): [{"@id": "urn:ngsi-ld:Dataset:a"}]});
    let error = refusal(&entity(&json!({"https://ex.org/speed": [instance.clone(), instance]})));
    assert!(
        matches!(&error, EntityRefusal::RepeatedInstance { attribute, dataset_id } if attribute.as_str() == "https://ex.org/speed" && dataset_id.as_str() == "urn:ngsi-ld:Dataset:a"),
        "{error:?}"
    );
}

#[test]
fn two_default_instances_are_refused() {
    let instance = json!({"@type": [PROPERTY], ngsi("hasValue"): [{"@value": 1}]});
    let error = refusal(&entity(&json!({"https://ex.org/speed": [instance.clone(), instance]})));
    assert!(matches!(error, EntityRefusal::MultipleDefaultInstances { .. }), "{error:?}");
}

#[test]
fn an_ngsi_ld_null_in_state_is_refused_wherever_it_stands() {
    let documents = [
        entity(&json!({"https://ex.org/speed": [{"@type": [PROPERTY], ngsi("hasValue"): [{"@value": "urn:ngsi-ld:null"}]}]})),
        entity(&json!({"https://ex.org/speed": [{"@type": [RELATIONSHIP], ngsi("hasObject"): [{"@id": "urn:ngsi-ld:null"}]}]})),
        entity(&json!({"https://ex.org/speed": [{"@value": "urn:ngsi-ld:null"}]})),
        entity(&json!({"https://ex.org/speed": [{"@type": [PROPERTY], ngsi("hasValue"): [{"https://ex.org/a": [{"@value": "urn:ngsi-ld:null"}]}]}]})),
        entity(&json!({"https://ex.org/speed": [{"@type": [PROPERTY], ngsi("hasValue"): [{"@value": 1}], ngsi("observedAt"): date_time("urn:ngsi-ld:null")}]})),
        entity(&json!({ngsi("scope"): [{"@value": "urn:ngsi-ld:null"}]})),
    ];
    for document in documents {
        let error = refusal(&document);
        assert!(
            matches!(
                error,
                EntityRefusal::ReservedNullValue { .. } | EntityRefusal::NestedNullValue { .. } | EntityRefusal::ScopeNull | EntityRefusal::NullInState { .. }
            ),
            "{document}: {error:?}"
        );
    }
}

#[test]
fn a_geometry_collection_is_refused_as_a_geoproperty_value() {
    let error = refusal(&entity(&json!({
        ngsi("location"): [{"@type": [GEO_PROPERTY], ngsi("hasValue"): [{"@type": [format!("{GEOJSON}GeometryCollection")], format!("{GEOJSON}geometries"): []}]}]
    })));
    assert!(
        matches!(
            &error,
            EntityRefusal::GeoProperty {
                source: GeometryError::GeometryCollectionUnsupported,
                ..
            }
        ),
        "{error:?}"
    );
}

/// Clause 3.1 makes the empty array a Property value: the stored `hasValue: []`
/// reads as a Property holding no value and is written back as it was.
#[test]
fn a_property_holding_the_empty_array_round_trips() {
    let document = entity(&json!({"https://ex.org/tags": [property(&json!([]))]}));
    let bytes = serde_json::to_vec(&document).expect("JSON");
    let state = EntityState::from_slice(&bytes).expect("a stored entity");
    let value = state
        .attributes()
        .get("https://ex.org/tags")
        .and_then(|instances| instances.get(None))
        .map(|instance| instance.value().clone());
    pretty_assertions::assert_eq!(
        value,
        Some(FamilyValue::Property {
            values: Vec::new(),
            unit_code: None,
            value_type: None,
        })
    );
    pretty_assertions::assert_eq!(round_trip(&document), document);
}

/// A Relationship names at least one target (clause 4.5.3.2), a
/// `VocabProperty` at least one term (clause 4.5.20.2) and a `GeoProperty` one
/// geometry (clause 4.7.1), so none of their members is read empty the way a
/// Property value is.
#[test]
fn an_empty_member_of_a_family_naming_at_least_one_item_is_refused() {
    for (family, member, shape) in [
        (RELATIONSHIP, "hasObject", MemberShape::Object),
        (VOCAB_PROPERTY, "hasVocab", MemberShape::Vocab),
        (GEO_PROPERTY, "hasValue", MemberShape::Value),
    ] {
        let error = refusal(&entity(&json!({"https://ex.org/a": [stamped(json!({"@type": [family], ngsi(member): []}))]})));
        assert!(
            matches!(error, EntityRefusal::Malformed { member: refused, .. } if refused == shape),
            "{member}: {error:?}"
        );
    }
}

#[test]
fn a_geoproperty_whose_value_is_no_geometry_is_refused() {
    for value in [
        json!([{"@value": 5}]),
        json!([{"https://ex.org/a": [{"@value": 1}]}]),
        json!([{"@type": "@json", "@value": {"type": "Point", "coordinates": [1, 2]}}]),
    ] {
        let error = refusal(&entity(&json!({ngsi("location"): [{"@type": [GEO_PROPERTY], ngsi("hasValue"): value}]})));
        assert!(
            matches!(
                error,
                EntityRefusal::GeoProperty { .. }
                    | EntityRefusal::Malformed {
                        member: MemberShape::Geometry,
                        ..
                    }
            ),
            "{value}: {error:?}"
        );
    }
}

#[test]
fn malformed_documents_are_refused_with_a_typed_refusal() {
    let cases = [
        (json!({"@id": 5}), "id"),
        (json!([]), "root"),
        (json!([{"@id": "urn:a:b", "@type": "https://ex.org/T"}]), "type"),
        (
            json!([{"@id": "urn:a:b", "@type": ["https://ex.org/T"], "https://ex.org/a": [{"@type": ["https://ex.org/NotAFamily"]}]}]),
            "family",
        ),
        (
            json!([{"@id": "urn:a:b", "@type": ["https://ex.org/T"], "https://ex.org/a": [{"@type": [PROPERTY]}]}]),
            "missing value",
        ),
        (
            json!([{"@id": "urn:a:b", "@type": ["https://ex.org/T"], "https://ex.org/a": [{"@type": [RELATIONSHIP], ngsi("hasValue"): [{"@value": 1}]}]}]),
            "forbidden",
        ),
        (
            json!([{"@id": "urn:a:b", "@type": ["https://ex.org/T"], "https://ex.org/a": [{"@type": [RELATIONSHIP], ngsi("hasObject"): [{"@id": "urn:x:y"}], ngsi("unitCode"): [{"@value": "KMH"}]}]}]),
            "unit code",
        ),
        (
            json!([{"@id": "urn:a:b", "@type": ["https://ex.org/T"], "https://ex.org/a": [{"@type": [PROPERTY], ngsi("hasValue"): [{"@value": 1}], ngsi("unitCode"): [{"@value": "km/h"}]}]}]),
            "unusable unit code",
        ),
        (json!([{"@id": "urn:a:b"}]), "no type"),
    ];
    for (document, case) in cases {
        let bytes = serde_json::to_vec(&document).expect("JSON");
        let error = EntityState::from_slice(&bytes).expect_err(case);
        let expected = match case {
            "id" => matches!(error, EntityRefusal::Syntax { .. }),
            "root" => matches!(error, EntityRefusal::Malformed { member: MemberShape::Root, .. }),
            "type" => matches!(error, EntityRefusal::TypeNotString),
            "family" => matches!(error, EntityRefusal::UnsupportedAttributeType),
            "missing value" => matches!(error, EntityRefusal::MissingValue { .. }),
            "forbidden" => matches!(error, EntityRefusal::MemberForbidden { .. }),
            "unit code" => matches!(error, EntityRefusal::UnitCodeForbidden { .. }),
            "unusable unit code" => matches!(error, EntityRefusal::UnitCodeUnusable { .. }),
            "no type" => matches!(error, EntityRefusal::MissingType),
            other => panic!("unknown case {other}"),
        };
        assert!(expected, "{case}: {error:?}");
    }
}

#[test]
fn a_refusal_never_echoes_the_raw_input() {
    let error = refusal(
        &json!([{"@id": "urn:a:b", "@type": ["https://ex.org/T"], "https://ex.org/a": [{"@type": [PROPERTY], ngsi("hasValue"): [{"@value": "<secret>", "@language": "not a tag!"}]}]}]),
    );
    assert!(!error.to_string().contains("<secret>"), "{error}");
    let syntax = EntityState::from_slice(br#"[{"@id": "<secret>"#).expect_err("not JSON");
    assert!(!syntax.to_string().contains("<secret>"), "{syntax}");
}

/// A `location` whose geometry node is `node`.
fn located(node: &Value) -> Value {
    entity(&json!({ngsi("location"): [stamped(json!({"@type": [GEO_PROPERTY], ngsi("hasValue"): [node]}))]}))
}

/// The expanded Point node at `(1, 2)`, with `members` beside its coordinates.
fn point_node(members: &Value) -> Value {
    let mut node = json!({"@type": [format!("{GEOJSON}Point")], COORDINATES: [{"@list": [{"@value": 1}, {"@value": 2}]}]});
    if let (Some(node), Some(members)) = (node.as_object_mut(), members.as_object()) {
        node.extend(members.iter().map(|(key, value)| (key.clone(), value.clone()))); // clone: the fixture is built from borrowed parts
    }
    node
}

/// IETF RFC 7946 sections 5 and 6.1, Annex B's `geojson:bbox` term, clause
/// 4.6.4: a stored geometry node keeps its box and its foreign members, read
/// and written back unchanged.
#[test]
fn a_stored_geometry_node_with_a_box_and_foreign_members_round_trips() {
    let document = located(&point_node(&json!({
        format!("{GEOJSON}bbox"): [{"@list": [{"@value": 1}, {"@value": 2}, {"@value": 1}, {"@value": 2}]}],
        "https://uri.etsi.org/ngsi-ld/default-context/title": [{"@value": "plot 7"}]
    })));
    pretty_assertions::assert_eq!(round_trip(&document), document);

    let state = EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity");
    let (_, instance) = state
        .attributes()
        .get(ngsi("location").as_str())
        .and_then(|member| member.default_instance())
        .expect("the location");
    let Some(GeoValue::Geometry(geometry)) = instance.value().geometry() else {
        panic!("a geometry node");
    };
    let members = geometry.members().expect("the members are kept");
    pretty_assertions::assert_eq!(members.bbox().map(|bbox| bbox.ordinates().collect::<Vec<_>>()), Some(vec![1.0, 2.0, 1.0, 2.0]));
    pretty_assertions::assert_eq!(
        members.foreign().keys().map(AttributeName::as_str).collect::<Vec<_>>(),
        vec!["https://uri.etsi.org/ngsi-ld/default-context/title"]
    );
}

/// RFC 7946 section 5: a box of three numbers is no box.
#[test]
fn a_stored_box_of_three_numbers_is_refused() {
    let document = located(&point_node(
        &json!({format!("{GEOJSON}bbox"): [{"@list": [{"@value": 1}, {"@value": 2}, {"@value": 3}]}]}),
    ));
    let refused = refusal(&document);
    assert!(
        matches!(
            refused,
            EntityRefusal::GeoProperty {
                source: GeometryError::InvalidBbox(BoundingBoxError::Length { members: 3 }),
                ..
            }
        ),
        "{refused:?}"
    );
}

/// Clause 4.5.0: entity state holds the NGSI-LD Null nowhere, a foreign
/// member of a geometry included.
#[test]
fn a_null_inside_a_foreign_member_is_refused() {
    for foreign in [
        json!([{"@value": "urn:ngsi-ld:null"}]),
        json!([{"https://ex.org/k": [{"@value": "urn:ngsi-ld:null"}]}]),
    ] {
        let document = located(&point_node(&json!({"https://ex.org/title": foreign})));
        let refused = refusal(&document);
        assert!(
            matches!(refused, EntityRefusal::NullBesideData { .. } | EntityRefusal::NullInState { .. }),
            "{foreign}: {refused:?}"
        );
    }
}

/// A geometry node holds one box.
#[test]
fn two_box_members_are_refused() {
    let bbox = format!(r#""{GEOJSON}bbox": [{{"@list": [{{"@value": 1}}, {{"@value": 2}}, {{"@value": 1}}, {{"@value": 2}}]}}]"#);
    let text = format!(
        r#"[{{"@id": "urn:ngsi-ld:Building:1", "@type": ["https://ex.org/Building"], "{location}": [{{"@type": ["{GEO_PROPERTY}"], "{value}": [{{"@type": ["{GEOJSON}Point"], "{COORDINATES}": [{{"@list": [{{"@value": 1}}, {{"@value": 2}}]}}], {bbox}, {bbox}}}]}}]}}]"#,
        location = ngsi("location"),
        value = ngsi("hasValue"),
    );
    let refused = EntityState::from_slice(text.as_bytes()).expect_err("two boxes");
    assert!(
        matches!(
            refused,
            EntityRefusal::Malformed {
                member: MemberShape::Geometry,
                ..
            }
        ),
        "{refused:?}"
    );
}

/// Every attribute a write is admitted with decodes back: at the deepest
/// nesting any limit may admit, the stored entity document of each shape
/// stays within `serde_json`'s reader, decodes as a state and as a written
/// document, encodes to the depth the ceiling was computed from, and measures
/// the nesting it was built with.
#[test]
fn an_attribute_at_the_decodable_ceiling_round_trips_in_every_shape() {
    use crate::document::EntityDocument;
    use andromeda_ngsi_ld_number::json_value::READER_NESTING_LIMIT;
    use andromeda_ngsi_ld_representation::attribute_nesting::AttributeNestingLimit;
    use andromeda_test_fixtures::nesting_fixture::{NESTED_ATTRIBUTE, NestingShape, container_depth, nested_instance, stored_entity};

    let ceiling = AttributeNestingLimit::DECODABLE_CEILING.get();
    for shape in NestingShape::ALL {
        let stored = serde_json::to_vec(&stored_entity("urn:ngsi-ld:NestingProbe:1", nested_instance(shape, ceiling))).expect("JSON");
        let state = EntityState::from_slice(&stored).unwrap_or_else(|refusal| panic!("{shape:?} at the ceiling decodes: {refusal}"));
        let encoded = state.to_vec().expect("the state encodes");
        let expected = match shape {
            NestingShape::JsonChain => ceiling + 6,
            NestingShape::ObjectChain | NestingShape::ListChain | NestingShape::SubAttributeChain => 2 * ceiling + 6,
        };
        pretty_assertions::assert_eq!(container_depth(&encoded), expected, "{shape:?}");
        assert!(
            container_depth(&encoded) + 3 <= READER_NESTING_LIMIT,
            "{shape:?} leaves room for the deepest queue"
        );
        pretty_assertions::assert_eq!(EntityState::from_slice(&encoded).expect("the encoding decodes"), state, "{shape:?}");
        let written = EntityDocument::from_slice(&encoded).expect("the encoding decodes as a written document");
        let attribute: AttributeName = NESTED_ATTRIBUTE.parse().expect("an attribute name");
        let nesting = written.attributes()[&attribute].instances()[0].nesting();
        pretty_assertions::assert_eq!(nesting.get(), ceiling, "{shape:?}");
    }
}
