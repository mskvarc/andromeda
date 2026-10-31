//! A delete of an unknown attribute after the history was deleted is resource not found.

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
    model::{AttributeName, Entity, ErrorType, NgsiLdMediaType, Value, VocabProperty, VocabTerm},
};

/// Pins: Delete Attribute humidity, which the Entity does not hold, after its Temporal Evolution was deleted
/// answers 404 `ResourceNotFound`
/// Clause: 5.6.5.4, 5.6.16.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, so an Entity created through the Core API has a
/// Temporal Evolution to delete
#[gtest]
#[tokio::test]
async fn o_att_del_9003_a_delete_of_an_unknown_attribute_after_the_history_was_deleted_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_unknown_attribute_id = namespace.urn("Building", "unknown-attribute")?;
    let building_type = namespace.entity_type("Building")?;
    let building_unknown_attribute = Entity::builder()
        .id(building_unknown_attribute_id.clone())
        .entity_type(building_type)
        .property("temperature", Value::Number("21.5".parse()?))
        .vocab_property("category", VocabProperty::builder("office".parse::<VocabTerm>()?))
        .build()?;
    broker.entities().create(&building_unknown_attribute).send().await?.done()?;
    broker.temporal().delete(&building_unknown_attribute_id).send().await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_unknown_attribute_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    let answer_2 = broker
        .with_responses()
        .entities()
        .delete_attribute(&building_unknown_attribute_id, &"humidity".parse::<AttributeName>()?)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
