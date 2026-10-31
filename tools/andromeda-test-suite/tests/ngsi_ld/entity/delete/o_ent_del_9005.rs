//! A repeated entity delete after the history was deleted is resource not found.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Value, VocabProperty, VocabTerm},
};

/// Pins: A second Delete Entity of an Entity deleted after its Temporal Evolution answers 404 `ResourceNotFound`
/// Clause: 5.6.6.4, 5.6.16.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, so an Entity created through the Core API has a
/// Temporal Evolution to delete
#[gtest]
#[tokio::test]
async fn o_ent_del_9005_a_repeated_entity_delete_after_the_history_was_deleted_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_deleted_twice_id = namespace.urn("Building", "deleted-twice")?;
    let building_type = namespace.entity_type("Building")?;
    let building_deleted_twice = Entity::builder()
        .id(building_deleted_twice_id.clone())
        .entity_type(building_type)
        .property("temperature", Value::Number("21.5".parse()?))
        .vocab_property("category", VocabProperty::builder("office".parse::<VocabTerm>()?))
        .build()?;
    broker.entities().create(&building_deleted_twice).send().await?.done()?;
    broker.temporal().delete(&building_deleted_twice_id).send().await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_deleted_twice_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    broker.entities().delete(&building_deleted_twice_id).send().await?.done()?;
    let answer_2 = broker.with_responses().entities().delete(&building_deleted_twice_id).send().await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
