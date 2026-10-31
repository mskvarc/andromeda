//! A merge after the history was deleted succeeds.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Value, VocabProperty, VocabTerm},
};

/// Pins: Merge Entity with temperature 18.25 on an Entity whose Temporal Evolution was deleted first answers 204
/// and the Entity holds 18.25
/// Clause: 5.6.16.4, 5.6.17.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, so an Entity created through the Core API has a
/// Temporal Evolution to delete
#[gtest]
#[tokio::test]
async fn o_ent_mrg_9022_a_merge_after_the_history_was_deleted_succeeds() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_merge_id = namespace.urn("Building", "merge")?;
    let building_type = namespace.entity_type("Building")?;
    let building_merge = Entity::builder()
        .id(building_merge_id.clone())
        .entity_type(building_type)
        .property("temperature", Value::Number("21.5".parse()?))
        .vocab_property("category", VocabProperty::builder("office".parse::<VocabTerm>()?))
        .build()?;
    broker.entities().create(&building_merge).send().await?.done()?;
    broker.temporal().delete(&building_merge_id).send().await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_merge_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    let fragment = EntityFragment::builder().property("temperature", Value::Number("18.25".parse()?)).build()?;
    broker.entities().merge(&building_merge_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_merge_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("temperature", any_attribute().value(Value::Number("18.25".parse()?))));
    Ok(())
}
