//! A json property takes the new json and keeps its sub attribute.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, attribute_of},
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeKind, AttributeName, Entity, JsonProperty, Lossless, NgsiLdMediaType, RawJson},
};
use serde_json::json;

/// Pins: Partial Attribute Update of a `JsonProperty` with a json member answers 204, the json becomes the one of the
/// Fragment and the sub-Property stays
/// Clause: 5.6.4.4, 5.5.8, 4.5.24
#[gtest]
#[tokio::test]
async fn n_att_pat_0024_a_json_property_takes_the_new_json_and_keeps_its_sub_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .json_property(
            "jsonProperty",
            JsonProperty::builder(format!(r#"{{"id": "original-{namespace}", "data": [1, 2, 3]}}"#).parse::<RawJson>()?).property("verified", true),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragment =
        Attribute::from(JsonProperty::builder(format!(r#"{{"id": "another-{namespace}", "data": [4, 5, 6]}}"#).parse::<RawJson>()?).build()?).into_patch();
    let answer = broker
        .with_responses()
        .entities()
        .update_attribute(&building_id, &"jsonProperty".parse::<AttributeName>()?, fragment)
        .form(Lossless::Concise)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "jsonProperty",
            attribute_of(AttributeKind::JsonProperty)
                .json(json!({"id": format!("another-{namespace}"), "data": [4, 5, 6]}))
                .sub_attribute("verified", any_attribute().value(true))
        )
    );
    Ok(())
}
