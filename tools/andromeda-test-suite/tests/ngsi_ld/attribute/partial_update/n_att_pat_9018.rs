//! A null key inside a json property is refused outside a merge.

use andromeda_test_suite::{attribute_matcher::attribute_of, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeKind, AttributeName, Entity, ErrorType, JsonProperty, NgsiLdMediaType, RawJson},
};
use serde_json::json;

/// Pins: Create Entity and Partial Attribute Update of a `JsonProperty` whose json holds urn:ngsi-ld:null under a key
/// answer 400 `BadRequestData`, and the held json is unchanged
/// Clause: 5.5.4, 4.5.24.2
#[gtest]
#[tokio::test]
async fn n_att_pat_9018_a_null_key_inside_a_json_property_is_refused_outside_a_merge() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_json_property_nested_refusal_id = namespace.urn("Building", "json-property-nested-refusal")?;
    let building_type = namespace.entity_type("Building")?;
    let building_json_property_nested_refusal = Entity::builder()
        .id(building_json_property_nested_refusal_id.clone())
        .entity_type(building_type.clone())
        .json_property(
            "jsonProperty",
            JsonProperty::builder(r#"{"anythingElse": "urn:ngsi-ld:null"}"#.parse::<RawJson>()?),
        )
        .build()?;
    let answer = broker.with_responses().entities().create(&building_json_property_nested_refusal).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_json_property_nested_refusal_2 = Entity::builder()
        .id(building_json_property_nested_refusal_id.clone())
        .entity_type(building_type)
        .json_property("jsonProperty", JsonProperty::builder(r#"{"anythingElse": 1}"#.parse::<RawJson>()?))
        .build()?;
    broker.entities().create(&building_json_property_nested_refusal_2).send().await?.done()?;
    let patch = Attribute::from(JsonProperty::builder(r#"{"anythingElse": "urn:ngsi-ld:null"}"#.parse::<RawJson>()?).build()?).into_patch();
    let answer_2 = broker
        .with_responses()
        .entities()
        .update_attribute(&building_json_property_nested_refusal_id, &"jsonProperty".parse::<AttributeName>()?, patch)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_json_property_nested_refusal_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_attribute("jsonProperty", attribute_of(AttributeKind::JsonProperty).json(json!({"anythingElse": 1})))
    );
    Ok(())
}
