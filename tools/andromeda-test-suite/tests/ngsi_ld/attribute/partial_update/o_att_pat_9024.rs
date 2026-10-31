//! A partial attribute update after the history was deleted succeeds.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Value, VocabProperty, VocabTerm},
};

/// Pins: Partial Attribute Update of temperature to 12.5 on an Entity whose Temporal Evolution was deleted first
/// answers 204 and the Entity holds 12.5
/// Clause: 5.6.4.4, 5.6.16.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, so an Entity created through the Core API has a
/// Temporal Evolution to delete
#[gtest]
#[tokio::test]
async fn o_att_pat_9024_a_partial_attribute_update_after_the_history_was_deleted_succeeds() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_partial_id = namespace.urn("Building", "partial")?;
    let building_type = namespace.entity_type("Building")?;
    let building_partial = Entity::builder()
        .id(building_partial_id.clone())
        .entity_type(building_type)
        .property("temperature", Value::Number("21.5".parse()?))
        .vocab_property("category", VocabProperty::builder("office".parse::<VocabTerm>()?))
        .build()?;
    broker.entities().create(&building_partial).send().await?.done()?;
    broker.temporal().delete(&building_partial_id).send().await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_partial_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    // A patch of a bare value is rendered as the value alone by the model, so it is sent as written.
    let request = json_request_to(
        &broker,
        Method::PATCH,
        &format!("entities/{building_partial_id}/attrs/temperature"),
        r#"{"value": 12.5}"#,
    )?;
    let answer_2 = broker.send_unchecked(request).await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_partial_id)
        .send()
        .await?;
    expect_that!(retrieved.body(), has_attribute("temperature", property().value(Value::Number("12.5".parse()?))));
    Ok(())
}
