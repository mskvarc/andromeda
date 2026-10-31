//! Create refuses a context embedded anywhere below the top level.

use andromeda_test_suite::{
    broker,
    core_context::core_context_list,
    namespace::Namespace,
    problem::{answered, problem},
    value::object,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson, Value},
};

/// Pins: Create Entity with an @context inside an attribute node, a sub-Attribute, a Property value object, a
/// `GeoProperty` geometry, a `LanguageProperty` language map or a Relationship's sub-Relationship answers 400
/// `BadRequestData` each time and stores nothing
/// Clause: 5.5.7, 4.5.1, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9015_create_refuses_a_context_embedded_anywhere_below_the_top_level() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let core_context_list_body = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let device_node_id = namespace.urn("Device", "node")?;
    let device_type = namespace.entity_type("Device")?;
    let device_sub_attribute_id = namespace.urn("Device", "sub-attribute")?;
    let device_value_object_id = namespace.urn("Device", "value-object")?;
    let device_geometry_id = namespace.urn("Device", "geometry")?;
    let device_language_map_id = namespace.urn("Device", "language-map")?;
    let device_relationship_sub_attribute_id = namespace.urn("Device", "relationship-sub-attribute")?;
    let device_node = Entity::builder().id(device_node_id.clone()).entity_type(device_type.clone()).build()?;
    // The model refuses it (member `@context` is not allowed here at speed), so `speed` is written as text.
    let speed: RawJson = format!(r#"{{"@context": {{"speed": "https://example.org/{namespace}/speed"}}, "type": "Property", "value": 1}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&device_node, Lossless::Normalized).set_member("speed", &speed);
    let request = core_context_list_body
        .unchecked(&CreateEntity::builder().entity(&device_node).build())?
        .document(&document);
    let answer = core_context_list_body.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_node_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let device_sub_attribute = Entity::builder().id(device_sub_attribute_id.clone()).entity_type(device_type.clone()).build()?;
    // The model refuses it (member `@context` is not allowed here at speed.accuracy), so `speed` is written as text.
    let speed_2: RawJson = format!(r#"{{"type": "Property", "value": 1, "accuracy": {{"@context": {{"accuracy": "https://example.org/{namespace}/accuracy"}}, "type": "Property", "value": 0.5}}}}"#).parse()?;
    let document_2 = UncheckedDocument::from_entity(&device_sub_attribute, Lossless::Normalized).set_member("speed", &speed_2);
    let request_2 = core_context_list_body
        .unchecked(&CreateEntity::builder().entity(&device_sub_attribute).build())?
        .document(&document_2);
    let answer_3 = core_context_list_body.send_unchecked(request_2).await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_sub_attribute_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let device_value_object = Entity::builder()
        .id(device_value_object_id.clone())
        .entity_type(device_type.clone())
        .property(
            "speed",
            object([
                (
                    "@context",
                    object([("reading", Value::from(format!("https://example.org/{namespace}/reading")))]),
                ),
                ("reading", Value::from(1)),
            ]),
        )
        .build()?;
    let answer_5 = core_context_list_body.with_responses().entities().create(&device_value_object).send().await;
    expect_that!(
        &answer_5,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_value_object_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    let device_geometry = Entity::builder().id(device_geometry_id.clone()).entity_type(device_type.clone()).build()?;
    // The model refuses it (a geometry with foreign members), so `location` is written as text.
    let location: RawJson = r#"{"type": "GeoProperty", "value": {"@context": {}, "type": "Point", "coordinates": [13.4, 52.5]}}"#.parse()?;
    let document_3 = UncheckedDocument::from_entity(&device_geometry, Lossless::Normalized).set_member("location", &location);
    let request_3 = core_context_list_body
        .unchecked(&CreateEntity::builder().entity(&device_geometry).build())?
        .document(&document_3);
    let answer_7 = core_context_list_body.send_unchecked(request_3).await;
    expect_that!(
        &answer_7,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    let answer_8 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_geometry_id)
        .send()
        .await;
    expect_that!(&answer_8, answered(StatusCode::NOT_FOUND));
    let device_language_map = Entity::builder().id(device_language_map_id.clone()).entity_type(device_type.clone()).build()?;
    // The model refuses it (invalid language map at label), so `label` is written as text.
    let label: RawJson = r#"{"type": "LanguageProperty", "languageMap": {"@context": {}, "en": "hello"}}"#.parse()?;
    let document_4 = UncheckedDocument::from_entity(&device_language_map, Lossless::Normalized).set_member("label", &label);
    let request_4 = core_context_list_body
        .unchecked(&CreateEntity::builder().entity(&device_language_map).build())?
        .document(&document_4);
    let answer_9 = core_context_list_body.send_unchecked(request_4).await;
    expect_that!(
        &answer_9,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    let answer_10 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_language_map_id)
        .send()
        .await;
    expect_that!(&answer_10, answered(StatusCode::NOT_FOUND));
    let device_relationship_sub_attribute = Entity::builder()
        .id(device_relationship_sub_attribute_id.clone())
        .entity_type(device_type.clone())
        .build()?;
    // The model refuses it (member `@context` is not allowed here at belongsTo.providedBy), so `belongsTo` is written as text.
    let belongs_to: RawJson = format!(r#"{{"type": "Relationship", "object": "urn:ngsi-ld:Site:{namespace}-1", "providedBy": {{"@context": {{}}, "type": "Relationship", "object": "urn:ngsi-ld:Sensor:{namespace}-1"}}}}"#).parse()?;
    let document_5 = UncheckedDocument::from_entity(&device_relationship_sub_attribute, Lossless::Normalized).set_member("belongsTo", &belongs_to);
    let request_5 = core_context_list_body
        .unchecked(&CreateEntity::builder().entity(&device_relationship_sub_attribute).build())?
        .document(&document_5);
    let answer_11 = core_context_list_body.send_unchecked(request_5).await;
    expect_that!(
        &answer_11,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("@context")
    );
    let answer_12 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_relationship_sub_attribute_id)
        .send()
        .await;
    expect_that!(&answer_12, answered(StatusCode::NOT_FOUND));
    Ok(())
}
