//! An attribute delete does not recreate the history.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, NgsiLdMediaType, Value, VocabProperty, VocabTerm},
};

/// Pins: Delete Attribute temperature after the Temporal Evolution was deleted answers 204 and Retrieve Temporal
/// Evolution still answers 404
/// Clause: 4.5.7, 4.5.8, 5.6.5.4, 5.6.16.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, so an Entity created through the Core API has a
/// Temporal Evolution to delete
#[gtest]
#[tokio::test]
async fn o_att_del_9004_an_attribute_delete_does_not_recreate_the_history() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_no_recreate_id = namespace.urn("Building", "no-recreate")?;
    let building_type = namespace.entity_type("Building")?;
    let building_no_recreate = Entity::builder()
        .id(building_no_recreate_id.clone())
        .entity_type(building_type)
        .property("temperature", Value::Number("21.5".parse()?))
        .vocab_property("category", VocabProperty::builder("office".parse::<VocabTerm>()?))
        .build()?;
    broker.entities().create(&building_no_recreate).send().await?.done()?;
    broker.temporal().delete(&building_no_recreate_id).send().await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_no_recreate_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    broker
        .entities()
        .delete_attribute(&building_no_recreate_id, &"temperature".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_no_recreate_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
