//! The clause 5.5.4 admission run through the entity service: entity
//! identity, NGSI-LD Null placement and `GeoProperty` geometry, as a request
//! reaches them.

use crate::{
    admission_fixture::{admitted_batch, admitted_context, admitted_entity, submitted},
    error::EntityError,
    merge_options::MergeOptions,
    notification_recording_harness::{ENTITY_ID, entity_uri, live_target, recording_service},
    side_effect::SideEffectPlacement,
    write_kind::{BatchCreate, BatchMerge, CreateEntity, MergeEntity},
    write_target::WriteTarget,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::{admission::null_sentinel_admission::NullSentinelAdmission, document::EntityDocument};
use andromeda_ngsi_ld_lexicon::identifier_length::IDENTIFIER_BYTE_LIMIT;
use andromeda_ngsi_ld_model::{
    attribute::{member_reading::MemberReading, merge_member::MergeMember},
    entity::{Entity, EntityPatch},
};
use serde_json::{Value, json};

/// One `Building` carrying `attribute`, as a client writes it before
/// expansion.
fn written_entity<M: MemberReading>(attribute: &str, node: &Value) -> Entity<M> {
    serde_json::from_value(json!({"id": ENTITY_ID, "type": "Building", attribute: node})).expect("the fixture is a valid entity")
}

/// The same payload as a fragment, for the operations that take one.
fn written_patch<M: MemberReading>(attribute: &str, node: &Value) -> EntityPatch<M> {
    serde_json::from_value(json!({attribute: node})).expect("the fixture is a valid entity fragment")
}

/// Decode an expanded document and screen it as a creation.
fn admit_expanded(expanded: &Value) -> Result<(), EntityError> {
    let document = EntityDocument::from_slice(&serde_json::to_vec(expanded).expect("JSON"))?;
    document.admit(NullSentinelAdmission::Refused)?;
    Ok(())
}

/// Decode a Building whose `attribute` is `node` as the transport does and
/// admit it as a creation, reporting the decode's refusal or the admission's.
async fn admit_created(attribute: &str, node: &Value) -> Result<(), String> {
    let (service, _recorded) = recording_service().await;
    let entity: Entity = serde_json::from_value(json!({"id": ENTITY_ID, "type": "Building", attribute: node})).map_err(|error| error.to_string())?;
    service
        .admit_entity_write::<CreateEntity>(&entity, &admitted_context(&service, &ContextSpec::core_only()).await)
        .await
        .map(drop)
        .map_err(|error| error.to_string())
}

/// Admit a Building located by `geometry` as a creation.
async fn admit_located(attribute: &str, geometry: &Value) -> Result<(), String> {
    admit_created(attribute, &json!({"type": "GeoProperty", "value": geometry})).await
}

/// One expanded entity carrying `location` as a `GeoProperty` in the fully
/// expanded node form, with the given coordinate member.
fn entity_with_expanded_node(coordinates: &Value) -> Value {
    json!([{
        "@id": "urn:ngsi-ld:Building:001",
        "@type": ["https://example.org/Building"],
        "https://uri.etsi.org/ngsi-ld/location": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{
                "@type": ["https://purl.org/geojson/vocab#Polygon"],
                "https://purl.org/geojson/vocab#coordinates": [coordinates]
            }]
        }]
    }])
}

/// An expanded coordinate scalar.
fn ordinate(value: f64) -> Value {
    json!({ "@value": value })
}

#[test]
fn a_fragment_without_an_identifier_passes() {
    let fragment = json!([{"https://example.org/member": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}]}]);
    let document = EntityDocument::from_slice(&serde_json::to_vec(&fragment).expect("JSON")).expect("a fragment names no entity");
    document
        .admit(NullSentinelAdmission::PartialUpdateFragment)
        .expect("a fragment names no entity");
}

#[test]
fn a_readable_identifier_and_types_pass() {
    admit_expanded(&json!([{"@id": "urn:ngsi-ld:Device:1", "@type": ["https://example.org/Device"]}])).expect("a well-formed entity");
}

/// A payload holds one entity node; a second one, whatever its identifier,
/// is no write the model admits.
#[test]
fn every_node_of_the_payload_is_screened() {
    let long_id = format!("urn:ngsi-ld:Device:{}", "a".repeat(IDENTIFIER_BYTE_LIMIT));
    let payload = json!([{"@id": "urn:ngsi-ld:Device:1", "@type": ["https://example.org/Device"]}, {"@id": long_id}]);
    let error = admit_expanded(&payload).expect_err("the payload is refused");
    assert!(matches!(error, EntityError::Validation(_)), "{error:?}");
}

/// Clause 5.5.4, third bullet: a null nested inside the JSON object on the
/// right-hand side of a Property `value` is `BadRequestData` outside a merge
/// fragment. Written against the real core `@context`, so the expansion the
/// checker reads is the one a request produces.
#[tokio::test]
async fn creating_an_entity_whose_property_value_object_nests_a_null_is_refused() {
    let (service, _recorded) = recording_service().await;
    let entity = written_entity("address", &json!({"type": "Property", "value": {"country": "urn:ngsi-ld:null"}}));
    let error = service
        .admit_entity_write::<CreateEntity>(&entity, &admitted_context(&service, &ContextSpec::core_only()).await)
        .await
        .expect_err("a creation may not nest the sentinel in a value object");
    assert!(matches!(error, EntityError::Validation(_)), "{error}");
}

/// Clause 5.5.12 EXAMPLE 2 merges exactly that fragment, so the same payload
/// the creation refuses must be accepted here.
#[tokio::test]
async fn merging_a_fragment_whose_property_value_object_nests_a_null_is_accepted() {
    let (service, _recorded) = recording_service().await;
    let patch = written_patch("address", &json!({"type": "Property", "value": {"country": "urn:ngsi-ld:null"}}));
    let admitted = admitted_entity::<MergeEntity>(&service, &patch, &ContextSpec::core_only()).await;
    service
        .merge(
            &live_target(),
            WriteTarget::new(&entity_uri(), None),
            &patch,
            admitted,
            &admitted_context(&service, &ContextSpec::core_only()).await,
            MergeOptions::default(),
        )
        .await
        .expect("clause 5.5.12 deletes a value-object key with the sentinel");
}

/// Clauses 5.5.8 and 5.5.12: a `datasetId` set to the sentinel is refused even
/// by the merge that every other null in the fragment is admitted for.
#[tokio::test]
async fn merging_a_fragment_that_deletes_a_dataset_id_is_refused() {
    let (service, _recorded) = recording_service().await;
    let patch = written_patch("category", &json!({"type": "Property", "value": 9, "datasetId": "urn:ngsi-ld:null"}));
    let error = service
        .admit_entity_write::<MergeEntity>(&patch, &admitted_context(&service, &ContextSpec::core_only()).await)
        .await
        .expect_err("a datasetId cannot be deleted by the sentinel");
    assert!(matches!(error, EntityError::Validation(_)), "{error}");
}

/// Clause 5.6.7.4 runs the clause 5.5.4 validation for each item of the array,
/// and clause 5.2.17 reports the failure against that entity alone.
#[tokio::test]
async fn a_batch_creation_refuses_the_sentinel_against_the_entity_that_carried_it() {
    let (service, _recorded) = recording_service().await;
    let entities = vec![written_entity("category", &json!({"type": "Property", "value": "urn:ngsi-ld:null"}))];
    let (admitted, refused) = service
        .admit_batch_write(BatchCreate, submitted(&service, entities, &ContextSpec::core_only()).await)
        .await;
    assert!(admitted.is_empty(), "nothing may be created");
    pretty_assertions::assert_eq!(refused.len(), 1);
    pretty_assertions::assert_eq!(refused[0].0.as_str(), ENTITY_ID);
    assert!(matches!(refused[0].1, EntityError::Validation(_)), "{}", refused[0].1);
}

/// Clause 5.6.20 is a merge, so the batch admits what clause 5.5.12 gives its
/// fragments.
#[tokio::test]
async fn a_batch_merge_admits_the_deletion_sentinel() {
    let (service, _recorded) = recording_service().await;
    let entities = vec![written_entity("category", &json!({"type": "Property", "value": "urn:ngsi-ld:null"}))];
    let admitted = admitted_batch(&service, BatchMerge, entities, &ContextSpec::core_only()).await;
    let batch = service
        .batch_merge(SideEffectPlacement::Inline(&live_target()), admitted)
        .await
        .expect("the batch itself completes");
    assert!(
        batch.errors.is_empty(),
        "{:?}",
        batch.errors.iter().map(|(id, error)| format!("{id}: {error}")).collect::<Vec<_>>()
    );
    pretty_assertions::assert_eq!(batch.success.len(), 1);
}

/// Clauses 4.5.21.2 and 4.5.22.2 give the list families their own spelling of
/// the sentinel, the array holding one single NGSI-LD Null of clause 4.5.0,
/// and clause 4.5.18 gives a `languageMap` the `{"@none": ...}` one. A
/// creation admits none of them, whichever wire spelling the Context Producer
/// wrote.
#[tokio::test]
async fn creating_an_entity_with_any_family_spelling_of_the_sentinel_is_refused() {
    let cases = [
        ("label", json!({"type": "LanguageProperty", "languageMap": {"@none": "urn:ngsi-ld:null"}})),
        ("tags", json!({"type": "ListProperty", "valueList": ["urn:ngsi-ld:null"]})),
        ("hasRooms", json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:null"]})),
        ("belongsTo", json!({"type": "Relationship", "object": "urn:ngsi-ld:null"})),
    ];
    for (attribute, node) in cases {
        let (service, _recorded) = recording_service().await;
        let outcome = service
            .admit_entity_write::<CreateEntity>(&written_entity(attribute, &node), &admitted_context(&service, &ContextSpec::core_only()).await)
            .await;
        match outcome {
            Ok(admitted) => panic!("{attribute}: a creation admits no NGSI-LD Null, yet {admitted:?} was admitted"),
            Err(error) => assert!(matches!(error, EntityError::Validation(_)), "{attribute}: {error}"),
        }
    }
}

#[tokio::test]
async fn a_two_dimensional_location_is_accepted() {
    admit_located("location", &json!({"type": "Point", "coordinates": [13.38, 52.52]}))
        .await
        .expect("a two-dimensional position");
}

/// RFC 7946 section 3.1.1 makes altitude the optional third ordinate, and
/// clause 4.7 adopts RFC 7946 whole, so a height is valid input.
#[tokio::test]
async fn a_three_dimensional_location_is_accepted() {
    admit_located("location", &json!({"type": "Point", "coordinates": [13.38, 52.52, 34.5]}))
        .await
        .expect("a three-dimensional position");
}

/// RFC 7946 nowhere requires a uniform arity within one geometry.
#[tokio::test]
async fn a_location_mixing_two_and_three_ordinate_positions_is_accepted() {
    admit_located("location", &json!({"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0, 50.0]]}))
        .await
        .expect("positions of either arity");
}

/// Clause 5.5.4: an unreadable `GeoProperty` is `BadRequestData`.
#[tokio::test]
async fn a_location_whose_position_carries_four_ordinates_is_rejected() {
    let error = admit_located("location", &json!({"type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0]}))
        .await
        .expect_err("four ordinates have no meaning");
    assert!(error.contains("4 ordinates"), "{error}");
}

/// RFC 7946 section 3.1.1 requires two or more elements; what matters for
/// clause 5.5.4 is that the write is refused.
#[tokio::test]
async fn a_location_whose_position_carries_one_ordinate_is_rejected() {
    admit_located("location", &json!({"type": "Point", "coordinates": [1.0]}))
        .await
        .expect_err("one ordinate is not a position");
}

#[test]
fn an_expanded_node_location_whose_position_carries_one_ordinate_is_rejected() {
    let entity = entity_with_expanded_node(&json!({"@list": [
        {"@list": [
            {"@list": [ordinate(0.0), ordinate(0.0)]},
            {"@list": [ordinate(4.0)]},
            {"@list": [ordinate(0.0), ordinate(0.0)]}
        ]}
    ]}));
    let error = admit_expanded(&entity).expect_err("one ordinate is not a position");
    assert!(error.to_string().contains("1 ordinates"), "got {error}");
}

/// The check recurses: the offending vertex is in an interior ring, four
/// levels of nesting below the coordinate member.
#[tokio::test]
async fn a_nested_polygon_vertex_with_four_ordinates_is_rejected() {
    let polygon = json!({
        "type": "Polygon",
        "coordinates": [
            [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]],
            [[1.0, 1.0], [2.0, 1.0], [2.0, 2.0, 5.0, 9.0], [1.0, 2.0], [1.0, 1.0]]
        ]
    });
    let error = admit_located("location", &polygon)
        .await
        .expect_err("an interior-ring vertex is still a position");
    assert!(error.contains("4 ordinates"), "got {error}");
}

/// Clause 4.7.1 lets any attribute be a `GeoProperty`; the three well-known ones
/// are not a closed list.
#[tokio::test]
async fn a_custom_geo_property_with_four_ordinates_is_rejected_and_named() {
    let error = admit_located("site", &json!({"type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0]}))
        .await
        .expect_err("a user-defined GeoProperty is validated too");
    assert!(error.contains("site"), "got {error}");
    assert!(error.contains("4 ordinates"), "got {error}");
}

#[tokio::test]
async fn an_operation_space_with_four_ordinates_is_rejected() {
    let error = admit_located("operationSpace", &json!({"type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0]}))
        .await
        .expect_err("operationSpace is a GeoProperty");
    assert!(error.contains("operationSpace"), "got {error}");
}

/// Clause 4.7.2 permits the `GeoJSON` document to arrive as a string.
#[tokio::test]
async fn a_geo_property_written_as_a_json_string_is_validated() {
    let error = admit_located("location", &json!("{\"type\":\"Point\",\"coordinates\":[1.0,2.0,3.0,4.0]}"))
        .await
        .expect_err("a stringified document is still a geometry");
    assert!(error.contains("4 ordinates"), "got {error}");
}

#[test]
fn a_geo_property_in_the_fully_expanded_node_form_is_validated() {
    let entity = entity_with_expanded_node(&json!({"@list": [
        {"@list": [
            {"@list": [ordinate(0.0), ordinate(0.0)]},
            {"@list": [ordinate(4.0), ordinate(0.0)]},
            {"@list": [ordinate(4.0), ordinate(4.0), ordinate(1.0), ordinate(9.0)]},
            {"@list": [ordinate(0.0), ordinate(0.0)]}
        ]}
    ]}));
    let error = admit_expanded(&entity).expect_err("the expanded node form is validated too");
    assert!(error.to_string().contains("4 ordinates"), "got {error}");
}

#[test]
fn a_valid_geo_property_in_the_fully_expanded_node_form_is_accepted() {
    let entity = entity_with_expanded_node(&json!({"@list": [
        {"@list": [
            {"@list": [ordinate(0.0), ordinate(0.0)]},
            {"@list": [ordinate(4.0), ordinate(0.0), ordinate(12.0)]},
            {"@list": [ordinate(4.0), ordinate(4.0)]},
            {"@list": [ordinate(0.0), ordinate(0.0)]}
        ]}
    ]}));
    admit_expanded(&entity).expect("a closed ring of valid positions");
}

/// Only instances typed `GeoProperty` are geometries to the expanded
/// admission; a Property whose expanded value happens to carry four numbers
/// is untouched.
#[test]
fn an_ordinary_property_carrying_a_coordinate_like_array_is_not_validated_as_a_geometry() {
    let entity = json!([{
        "@id": "urn:ngsi-ld:Building:001",
        "@type": ["https://example.org/Building"],
        "https://example.org/readings": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{
                "@type": ["https://purl.org/geojson/vocab#Point"],
                "https://purl.org/geojson/vocab#coordinates": [{"@list": [ordinate(1.0), ordinate(2.0), ordinate(3.0), ordinate(4.0)]}]
            }]
        }]
    }]);
    admit_expanded(&entity).expect("a Property value is no geometry");
}

/// Clause 4.5.5: the default instance passing is not enough, because a
/// datasetId-tagged instance is read back just as often.
#[tokio::test]
async fn every_instance_of_a_multi_instance_geo_property_is_validated() {
    let instances = json!([
        {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}},
        {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0]}, "datasetId": "urn:ngsi-ld:Dataset:A"}
    ]);
    let error = admit_created("location", &instances).await.expect_err("a tagged instance is validated too");
    assert!(error.contains("4 ordinates"), "got {error}");
}

/// Merge and partial update write a fragment rather than a whole entity, and
/// the fragment's geometries are read all the same.
#[test]
fn a_patch_fragment_is_validated() {
    let error = serde_json::from_value::<EntityPatch<MergeMember>>(json!({
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0]}}
    }))
    .expect_err("a fragment is validated");
    assert!(error.to_string().contains("4 ordinates"), "got {error}");
}

/// Clause 4.7.1: every RFC 7946 geometry except `GeometryCollection`.
#[tokio::test]
async fn a_location_that_is_a_geometry_collection_is_rejected() {
    let collection = json!({
        "type": "GeometryCollection",
        "geometries": [
            {"type": "Point", "coordinates": [1.0, 2.0]},
            {"type": "LineString", "coordinates": [[3.0, 4.0], [5.0, 6.0]]}
        ]
    });
    let error = admit_located("location", &collection)
        .await
        .expect_err("a collection is not an allowed GeoProperty geometry");
    assert!(error.contains("GeometryCollection"), "got {error}");
}

#[tokio::test]
async fn a_custom_geo_property_that_is_a_geometry_collection_is_rejected() {
    let error = admit_located("site", &json!({"type": "GeometryCollection", "geometries": []}))
        .await
        .expect_err("a user-defined GeoProperty is validated too");
    assert!(error.contains("GeometryCollection"), "got {error}");
}

#[test]
fn a_geometry_collection_in_the_fully_expanded_node_form_is_rejected() {
    let entity = json!([{
        "@id": "urn:ngsi-ld:Building:001",
        "@type": ["https://example.org/Building"],
        "https://uri.etsi.org/ngsi-ld/location": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{
                "@type": ["https://purl.org/geojson/vocab#GeometryCollection"],
                "https://purl.org/geojson/vocab#geometries": [
                    {"@list": [{
                        "@type": ["https://purl.org/geojson/vocab#Point"],
                        "https://purl.org/geojson/vocab#coordinates": [{"@list": [ordinate(1.0), ordinate(2.0)]}]
                    }]}
                ]
            }]
        }]
    }]);
    let error = admit_expanded(&entity).expect_err("the expanded node form is validated too");
    assert!(matches!(error, EntityError::Validation(_)), "{error:?}");
}

/// Clause 4.7.2 permits the `GeoJSON` document to arrive as a string.
#[tokio::test]
async fn a_geometry_collection_written_as_a_json_string_is_rejected() {
    let error = admit_located("location", &json!("{\"type\":\"GeometryCollection\",\"geometries\":[]}"))
        .await
        .expect_err("a stringified collection is still a collection");
    assert!(error.contains("GeometryCollection"), "got {error}");
}

#[test]
fn an_entity_carrying_no_geo_property_is_accepted() {
    admit_expanded(&json!([{"@id": "urn:ngsi-ld:Building:001", "@type": ["https://example.org/Building"]}])).expect("nothing to read as a geometry");
}

/// The NGSI-LD Null in place of a whole member deletes it only in a
/// partial update or a merge; a creation refuses it (clause 5.2.1).
#[tokio::test]
async fn a_bare_null_member_is_refused_on_create() {
    let error = admit_created("owner", &json!("urn:ngsi-ld:null"))
        .await
        .expect_err("a creation admits no NGSI-LD Null");
    assert!(error.contains("urn:ngsi-ld:null"), "{error}");
}

/// A `GeoProperty` whose value is the NGSI-LD Null is a deletion, which
/// a creation refuses (clause 5.2.1).
#[tokio::test]
async fn a_geo_property_null_value_is_refused_on_create() {
    let error = admit_created("location", &json!({"type": "GeoProperty", "value": "urn:ngsi-ld:null"}))
        .await
        .expect_err("a creation admits no NGSI-LD Null");
    assert!(error.contains("urn:ngsi-ld:null"), "{error}");
}

/// Outside a merge, the NGSI-LD Null on an instance member is no value
/// of that member, refused where it stands (clause 5.2.1).
#[tokio::test]
async fn a_null_on_an_instance_member_is_refused_outside_a_merge() {
    let error = admit_created("temperature", &json!({"type": "Property", "value": 1, "observedAt": "urn:ngsi-ld:null"}))
        .await
        .expect_err("observedAt holds a date-time");
    assert!(error.contains("/temperature/observedAt"), "{error}");
}
