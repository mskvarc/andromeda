//! A history delete after the entity was deleted succeeds.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Value, VocabProperty, VocabTerm},
};

/// Pins: Delete Temporal Evolution of an Entity whose current state was deleted first answers 204
/// Clause: 5.6.6.4, 5.6.16.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, so an Entity created through the Core API has a
/// Temporal Evolution to delete
#[gtest]
#[tokio::test]
async fn o_tmp_del_9001_a_history_delete_after_the_entity_was_deleted_succeeds() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_history_after_id = namespace.urn("Building", "history-after")?;
    let building_type = namespace.entity_type("Building")?;
    let building_history_after = Entity::builder()
        .id(building_history_after_id.clone())
        .entity_type(building_type)
        .property("temperature", Value::Number("21.5".parse()?))
        .vocab_property("category", VocabProperty::builder("office".parse::<VocabTerm>()?))
        .build()?;
    broker.entities().create(&building_history_after).send().await?.done()?;
    broker.entities().delete(&building_history_after_id).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_history_after_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    broker.temporal().delete(&building_history_after_id).send().await?;
    Ok(())
}
