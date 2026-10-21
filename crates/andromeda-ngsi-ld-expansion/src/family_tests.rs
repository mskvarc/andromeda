//! The eight attribute families, their members and the entity's own members,
//! expanded by the typed walk and by the processor alike (clauses 4.5, 4.7,
//! 4.18, 4.22, 5.2.4 to 5.2.38).

use crate::{
    expansion_fixture::{attribute, engine, entity, patch},
    normalization::Normalization,
};
use andromeda_ngsi_ld_entity::{
    document::{
        EntityDocument,
        attribute_member::AttributeMember,
        instance_document::InstanceDocument,
        slot::Slot,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::ExpandedValue,
    geo_value::GeoValue,
    list_object::ListObject,
    literal::LiteralValue,
    value_mode::{Fragment, FragmentMember, FragmentText},
};
use andromeda_ngsi_ld_geometry::geometry_type::GeometryType;
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, time::date_time::NgsiLdDateTime};
use andromeda_ngsi_ld_number::JsonNumber;
use serde_json::json;

/// The core context's default vocabulary, which every undefined term expands
/// against (clause 5.5.7).
const DEFAULT: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

/// The one instance written for `attribute`.
fn instance<'a>(document: &'a EntityDocument, attribute: &str) -> &'a InstanceDocument {
    let member = document.attributes().get(attribute).unwrap_or_else(|| panic!("{attribute} is written"));
    let [instance] = member.instances() else {
        panic!("{attribute} carries one instance");
    };
    instance
}

/// The expanded IRI of a term the core context does not define.
fn term(name: &str) -> String {
    format!("{DEFAULT}{name}")
}

/// The written Property value of an instance.
fn values(instance: &InstanceDocument) -> &[ExpandedValue<Fragment>] {
    let ValueSlot::Set(WrittenValue::Value(values)) = instance.value() else {
        panic!("a Property value, got {:?}", instance.value());
    };
    values.as_slice()
}

/// The number of a literal value.
fn number(value: &ExpandedValue<Fragment>) -> &JsonNumber {
    let ExpandedValue::Literal(literal) = value else {
        panic!("a literal, got {value:?}");
    };
    let LiteralValue::Number(number) = literal.value() else {
        panic!("a number, got {literal:?}");
    };
    number
}

/// Clause 4.5.2: a Property with its unit, observation time and dataset.
#[tokio::test]
async fn a_property_expands_to_its_value_unit_observation_and_dataset() {
    let document = entity(
        &engine(),
        json!({
            "id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle",
            "speed": {"type": "Property", "value": 21.5, "unitCode": "KMH", "observedAt": "2024-01-01T00:00:00Z", "datasetId": "urn:ngsi-ld:Dataset:a"}
        }),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();

    pretty_assertions::assert_eq!(document.id().map(ToString::to_string).as_deref(), Some("urn:ngsi-ld:Vehicle:1"));
    pretty_assertions::assert_eq!(document.types().iter().map(ToString::to_string).collect::<Vec<_>>(), vec![term("Vehicle")]);
    let speed = instance(&document, &term("speed"));
    pretty_assertions::assert_eq!(speed.family(), Some(AttributeType::Property));
    pretty_assertions::assert_eq!(number(&values(speed)[0]), &"21.5".parse::<JsonNumber>().expect("a number"));
    pretty_assertions::assert_eq!(speed.unit_code().and_then(Slot::as_set).map(|unit| unit.as_str()), Some("KMH"));
    pretty_assertions::assert_eq!(
        speed.observed_at().and_then(Slot::as_set).map(NgsiLdDateTime::to_string).as_deref(),
        Some("2024-01-01T00:00:00Z")
    );
    pretty_assertions::assert_eq!(speed.dataset_id().map(ToString::to_string).as_deref(), Some("urn:ngsi-ld:Dataset:a"));
}

/// Clause 4.5.5: `"datasetId": "@none"` is the default Attribute instance, so
/// it expands to an instance with no `datasetId`, on every family and beside a
/// named instance.
#[tokio::test]
async fn a_dataset_id_none_is_the_default_instance() {
    let document = entity(
        &engine(),
        json!({
            "id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 1, "datasetId": "@none"},
                {"type": "Property", "value": 2, "datasetId": "urn:ngsi-ld:Dataset:a"}
            ],
            "owner": {"type": "Relationship", "object": "urn:ngsi-ld:Person:1", "datasetId": "@none"}
        }),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();

    let owner = instance(&document, &term("owner"));
    assert!(owner.dataset_id().is_none());
    let speed = document.attributes().get(term("speed").as_str()).expect("speed is written");
    pretty_assertions::assert_eq!(
        speed
            .instances()
            .iter()
            .map(|instance| instance.dataset_id().map(ToString::to_string))
            .collect::<Vec<_>>(),
        vec![None, Some("urn:ngsi-ld:Dataset:a".to_owned())]
    );
}

/// Every family in one entity, each with a sub-attribute nested twice.
#[tokio::test]
async fn every_family_expands_identically_on_both_paths() {
    let nested = json!({"type": "Property", "value": 1, "accuracy": {"type": "Property", "value": 0.5, "source": {"type": "Relationship", "object": "urn:ngsi-ld:Sensor:1"}}});
    let document = entity(
        &engine(),
        json!({
            "id": "urn:ngsi-ld:Thing:1", "type": ["Thing", "Device"],
            "property": {"type": "Property", "value": "text", "valueType": "Text", "reading": nested},
            "relationship": {"type": "Relationship", "object": ["urn:ngsi-ld:A:1", "urn:ngsi-ld:A:2"], "objectType": "Building"},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.5]}},
            "label": {"type": "LanguageProperty", "languageMap": {"en": "hello", "de": ["hallo", "servus"], "@none": "hi"}},
            "category": {"type": "VocabProperty", "vocab": ["commercial", "urn:ngsi-ld:Category:1"]},
            "settings": {"type": "JsonProperty", "json": {"k": [1, 2.50, {"n": null}]}},
            "readings": {"type": "ListProperty", "valueList": [1, "two", {"three": 3}]},
            "parts": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Part:1", "urn:ngsi-ld:Part:2"]}
        }),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();

    pretty_assertions::assert_eq!(document.attributes().len(), 8);
    for family in AttributeType::ALL {
        assert!(
            document.attributes().values().any(|member| member.instances()[0].family() == Some(family)),
            "{family}"
        );
    }
    let relationship = instance(&document, &term("relationship"));
    let ValueSlot::Set(WrittenValue::Object(objects)) = relationship.value() else {
        panic!("Relationship targets");
    };
    pretty_assertions::assert_eq!(
        objects.iter().map(ToString::to_string).collect::<Vec<_>>(),
        vec!["urn:ngsi-ld:A:1", "urn:ngsi-ld:A:2"]
    );
    pretty_assertions::assert_eq!(
        relationship
            .object_types()
            .and_then(Slot::as_set)
            .map(|types| types.iter().map(ToString::to_string).collect::<Vec<_>>()),
        Some(vec![term("Building")])
    );
    let label = instance(&document, &term("label"));
    let ValueSlot::Set(WrittenValue::LanguageMap(entries)) = label.value() else {
        panic!("a language map");
    };
    pretty_assertions::assert_eq!(entries.entries().len(), 4);
    let parts = instance(&document, &term("parts"));
    pretty_assertions::assert_eq!(
        parts.value(),
        &ValueSlot::Set(WrittenValue::ObjectList(vec![
            ListObject::Wrapped("urn:ngsi-ld:Part:1".parse().expect("a URI")),
            ListObject::Wrapped("urn:ngsi-ld:Part:2".parse().expect("a URI")),
        ]))
    );
    let reading = &instance(&document, &term("property")).sub_attributes()[term("reading").as_str()];
    let [reading] = reading.instances() else {
        panic!("one reading");
    };
    let accuracy = &reading.sub_attributes()[term("accuracy").as_str()].instances()[0];
    assert!(accuracy.sub_attributes().contains_key(term("source").as_str()));
}

/// Clause 4.7: the geometry the transport read becomes the positions the
/// model holds, for a Point and for a Polygon.
#[tokio::test]
async fn a_geo_property_keeps_the_positions_the_transport_read() {
    let document = entity(
        &engine(),
        json!({
            "id": "urn:ngsi-ld:Site:1", "type": "Site",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.5, 34.0]}},
            "area": {"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]}}
        }),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();

    let ValueSlot::Set(WrittenValue::Geometry(GeoValue::Geometry(point))) = instance(&document, "https://uri.etsi.org/ngsi-ld/location").value() else {
        panic!("a geometry");
    };
    pretty_assertions::assert_eq!(point.form().geometry_type(), GeometryType::Point);
    let ValueSlot::Set(WrittenValue::Geometry(GeoValue::Geometry(area))) = instance(&document, &term("area")).value() else {
        panic!("a geometry");
    };
    pretty_assertions::assert_eq!(area.form().position_count().get(), 4);
}

/// Clause 4.7.2: a geometry written as a JSON string stays its text.
#[tokio::test]
async fn a_string_encoded_geometry_stays_its_text() {
    let text = r#"{"type":"Point","coordinates":[1,2]}"#;
    let document = entity(
        &engine(),
        json!({"id": "urn:ngsi-ld:Site:2", "type": "Site", "location": {"type": "GeoProperty", "value": text}}),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();

    let ValueSlot::Set(WrittenValue::Geometry(GeoValue::Encoded(encoded))) = instance(&document, "https://uri.etsi.org/ngsi-ld/location").value() else {
        panic!("an encoded geometry");
    };
    pretty_assertions::assert_eq!(encoded.as_str(), text);
}

/// A Property whose value is `GeoJSON`-shaped is a structured value: the `GeoJSON`
/// node expansion makes of it.
#[tokio::test]
async fn a_property_holding_a_geometry_is_the_geojson_node() {
    let document = entity(
        &engine(),
        json!({"id": "urn:ngsi-ld:Site:3", "type": "Site", "shape": {"type": "Property", "value": {"type": "LineString", "coordinates": [[1.0, 2.0], [3.5, 4.25]]}}}),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();

    let [ExpandedValue::Node(node)] = values(instance(&document, &term("shape"))) else {
        panic!("one node");
    };
    pretty_assertions::assert_eq!(node.types()[0].as_str(), "https://purl.org/geojson/vocab#LineString");
}

/// Clauses 4.6.3 and 4.6.4: numbers keep their exact value.
#[tokio::test]
async fn numbers_keep_their_exact_value() {
    let body = r#"{"id": "urn:ngsi-ld:Device:n", "type": "Device", "reading": {"type": "Property", "value": [0.100000000000000005, 123456789012345678901234567890, 1e-400, -0, 1.50]}}"#;
    let document = entity(&engine(), serde_json::from_str(body).expect("exact JSON"), Normalization::Entity)
        .await
        .expect("the fixture expands")
        .agreed();

    let numbers: Vec<String> = values(instance(&document, &term("reading")))
        .iter()
        .map(|value| number(value).to_string())
        .collect();
    let expected: Vec<String> = ["0.100000000000000005", "123456789012345678901234567890", "1e-400", "0", "1.5"]
        .iter()
        .map(|text| text.parse::<JsonNumber>().expect("a number").to_string())
        .collect();
    pretty_assertions::assert_eq!(numbers, expected);
}

/// Clauses 4.18 and 4.22: scope and the entity's expiry.
#[tokio::test]
async fn scope_and_expiry_are_entity_members() {
    let document = entity(
        &engine(),
        json!({"id": "urn:ngsi-ld:Device:s", "type": "Device", "scope": ["/a/b", "/c"], "expiresAt": "2030-01-01T00:00:00Z"}),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();

    let Some(Slot::Set(scopes)) = document.scope() else {
        panic!("scopes");
    };
    pretty_assertions::assert_eq!(scopes.as_slice().iter().map(ToString::to_string).collect::<Vec<_>>(), vec!["/a/b", "/c"]);
    pretty_assertions::assert_eq!(
        document.expires_at().and_then(Slot::as_set).map(NgsiLdDateTime::to_string).as_deref(),
        Some("2030-01-01T00:00:00Z")
    );
    assert!(document.attributes().is_empty());
}

/// Clause 4.5.2.3 and siblings: a concise entity is rewritten before it is
/// expanded, families inferred from their members.
#[tokio::test]
async fn a_concise_entity_is_rewritten_before_it_is_expanded() {
    let document = entity(
        &engine(),
        json!({
            "id": "urn:ngsi-ld:Device:c", "type": "Device",
            "speed": 42, "owner": {"object": "urn:ngsi-ld:Person:1"},
            "location": {"type": "Point", "coordinates": [1.0, 2.0]},
            "name": {"languageMap": {"en": "n"}},
            "measured": {"value": 7, "unitCode": "CEL", "accuracy": 0.5}
        }),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();

    pretty_assertions::assert_eq!(instance(&document, &term("speed")).family(), Some(AttributeType::Property));
    pretty_assertions::assert_eq!(instance(&document, &term("owner")).family(), Some(AttributeType::Relationship));
    pretty_assertions::assert_eq!(
        instance(&document, "https://uri.etsi.org/ngsi-ld/location").family(),
        Some(AttributeType::GeoProperty)
    );
    pretty_assertions::assert_eq!(instance(&document, &term("name")).family(), Some(AttributeType::LanguageProperty));
    let measured = instance(&document, &term("measured"));
    assert!(measured.sub_attributes().contains_key(term("accuracy").as_str()));
}

/// A structured Property value is a node of expanded members (clause 4.9
/// paths read it by expanded name).
#[tokio::test]
async fn a_structured_value_is_a_node_of_expanded_members() {
    let document = patch(
        &engine(),
        json!({"address": {"type": "Property", "value": {"street": "Main", "geo": {"lat": 1, "tags": ["a", "b"]}}}}),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();

    let [ExpandedValue::Node(address)] = values(instance(&document, &term("address"))) else {
        panic!("one node");
    };
    let Some(FragmentMember::Values(geo)) = address.members().get(term("geo").as_str()) else {
        panic!("the nested member");
    };
    let [ExpandedValue::Node(geo)] = geo.as_slice() else {
        panic!("a nested node");
    };
    let Some(FragmentMember::Values(tags)) = geo.members().get(term("tags").as_str()) else {
        panic!("the array member");
    };
    pretty_assertions::assert_eq!(tags.len(), 2);
}

/// Clause 4.5.22.3: the compact model reads a concise `objectList` into the
/// Relationship objects Table 5.2.37-1 normalizes it to, which expand to the
/// wrapped items the core context types `@id`, on both paths.
#[tokio::test]
async fn a_concise_object_list_expands_to_wrapped_items() {
    let document = patch(
        &engine(),
        json!({"id": "urn:ngsi-ld:Thing:1", "type": "Thing", "parts": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Part:1", "urn:ngsi-ld:Part:2"]}}),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();

    pretty_assertions::assert_eq!(
        instance(&document, &term("parts")).value(),
        &ValueSlot::Set(WrittenValue::ObjectList(vec![
            ListObject::Wrapped("urn:ngsi-ld:Part:1".parse().expect("a URI")),
            ListObject::Wrapped("urn:ngsi-ld:Part:2".parse().expect("a URI")),
        ]))
    );
}

/// A merge (clause 5.5.12) written as a typed fragment: no `id`, no `type`.
#[tokio::test]
async fn a_merge_fragment_names_no_entity() {
    let document = patch(
        &engine(),
        json!({"speed": {"type": "Property", "value": 1}, "label": {"type": "LanguageProperty", "languageMap": {"en": "urn:ngsi-ld:null"}}}),
        Normalization::MemberDeletions,
    )
    .await
    .expect("the fixture expands")
    .agreed();

    assert!(document.id().is_none() && document.types().is_empty());
    let ValueSlot::Set(WrittenValue::LanguageMap(entries)) = instance(&document, &term("label")).value() else {
        panic!("a language map");
    };
    pretty_assertions::assert_eq!(entries.entries()[0].text(), &FragmentText::Remove);
}

/// Clauses 5.6.4 and 5.6.19: one attribute instance under the name the
/// request path carries.
#[tokio::test]
async fn an_attribute_fragment_is_one_instance_under_its_expanded_name() {
    let engine = engine();
    let partial = attribute(&engine, "speed", json!({"value": 5, "observedAt": "2024-01-01T00:00:00Z"}))
        .await
        .expect("the fixture expands")
        .agreed();
    pretty_assertions::assert_eq!(partial.name().as_str(), term("speed"));
    // The compact model reads `value` as the member of a Property (clause 4.5.2.3).
    pretty_assertions::assert_eq!(partial.instance().family(), Some(AttributeType::Property));

    let replaced = attribute(&engine, "speed", json!({"type": "Property", "value": 7, "datasetId": "urn:ngsi-ld:Dataset:b"}))
        .await
        .expect("the fixture expands")
        .agreed();
    pretty_assertions::assert_eq!(replaced.instance().family(), Some(AttributeType::Property));
    assert!(replaced.instance().dataset_id().is_some());
}

/// Clause 4.5.5: one member may hold several instances, one per dataset.
#[tokio::test]
async fn a_multi_instance_member_keeps_its_instances_in_order() {
    let document = entity(
        &engine(),
        json!({
            "id": "urn:ngsi-ld:Device:m", "type": "Device",
            "temperature": [
                {"type": "Property", "value": 21},
                {"type": "Property", "value": 22, "datasetId": "urn:ngsi-ld:Dataset:b"}
            ]
        }),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();

    let AttributeMember::Instances(instances) = &document.attributes()[term("temperature").as_str()] else {
        panic!("instances");
    };
    pretty_assertions::assert_eq!(
        instances.iter().map(|instance| instance.dataset_id().is_some()).collect::<Vec<_>>(),
        vec![false, true]
    );
}

/// Clause 4.5.24.2: `json` is "a raw JSON object (or array of objects)" that
/// is "never subject to JSON-LD term expansion or compaction", and the core
/// context types it `@json`, so an array of any length is one JSON literal
/// holding the array as written.
#[tokio::test]
async fn a_json_array_expands_to_one_literal_holding_the_array() {
    for written in [json!([{"id": "a"}]), json!([{"id": "a"}, {"id": "b"}]), json!([1, "two"]), json!([])] {
        let document = entity(
            &engine(),
            json!({"id": "urn:ngsi-ld:Thing:j", "type": "Thing", "tickets": {"type": "JsonProperty", "json": written}}),
            Normalization::Entity,
        )
        .await
        .expect("the fixture expands")
        .agreed();
        let ValueSlot::Set(WrittenValue::Json(literals)) = instance(&document, &term("tickets")).value() else {
            panic!("a JSON value for {written}");
        };
        pretty_assertions::assert_eq!(
            literals
                .as_slice()
                .iter()
                .map(|literal| serde_json::from_str(literal.text()).expect("JSON"))
                .collect::<Vec<serde_json::Value>>(),
            vec![written.clone()],
            "{written}"
        );
    }
}
