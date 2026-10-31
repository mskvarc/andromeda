//! Append and replace refuse the null under a key of a property value.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    value::object,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::ReplaceEntity},
    model::{Attribute, Entity, EntityFragment, ErrorType, Lossless, NgsiLdMediaType, Property, RawJson, Value, ValuePatch},
};

/// Pins: Append Attributes, Replace Attribute and Replace Entity whose Property value holds urn:ngsi-ld:null under
/// a key each answer 400 `BadRequestData` and change nothing
/// Clause: 5.5.4, 5.6.3, 5.6.18.4, 5.6.19
#[gtest]
#[tokio::test]
async fn n_att_app_9005_append_and_replace_refuse_the_null_under_a_key_of_a_property_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_append_replace_nested_null_id = namespace.urn("Building", "append-replace-nested-null")?;
    let building_type = namespace.entity_type("Building")?;
    let building_append_replace_nested_null = Entity::builder()
        .id(building_append_replace_nested_null_id.clone())
        .entity_type(building_type.clone())
        .property(
            "address",
            object([
                ("street", Value::from("Strasse des 17. Juni")),
                ("city", Value::from("Berlin")),
                ("country", Value::from("Germany")),
            ]),
        )
        .build()?;
    broker.entities().create(&building_append_replace_nested_null).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .patch("office", Attribute::from(Property::merge_builder(ValuePatch::new().delete("country")).build()?))
        .build()?;
    let answer = broker
        .with_responses()
        .entities()
        .append_attributes(&building_append_replace_nested_null_id, &fragment)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses this attribute, so the body is sent as written.
    let request_2 = request_to(
        &broker,
        Method::PUT,
        &format!("entities/{building_append_replace_nested_null_id}/attrs/address"),
    )?
    .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
    .body(Bytes::from(
        "{\n    \"type\": \"Property\",\n    \"value\": {\n        \"country\": \"urn:ngsi-ld:null\"\n    }\n}",
    ));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let replacement = Entity::builder()
        .id(building_append_replace_nested_null_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (a request cannot hold the NGSI-LD Null in a member of `value` at address), so `address` is written as text.
    let address: RawJson = r#"{"type": "Property", "value": {"country": "urn:ngsi-ld:null"}}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&replacement, Lossless::Normalized).set_member("address", &address);
    let request_3 = broker.unchecked(&ReplaceEntity::builder().entity(&replacement).build())?.document(&document_2);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_append_replace_nested_null_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("office"));
    expect_that!(entity, has_attribute("address", any_attribute().value_member(["country"], "Germany")));
    Ok(())
}
