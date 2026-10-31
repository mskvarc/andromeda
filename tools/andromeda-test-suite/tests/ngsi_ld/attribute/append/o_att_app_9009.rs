//! A write after the history was deleted records a new instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, lacks_evolution},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, OneOrMany, Value, VocabProperty, VocabTerm},
    query::temporal_query::TemporalProperty,
};

/// Pins: Append Attributes humidity 44 after the Temporal Evolution was deleted records a Temporal Evolution of the
/// Entity type holding the one humidity instance and no temperature
/// Clause: 4.5.6, 4.5.7, 5.6.3.4, 5.6.16.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, so an Entity created through the Core API has a
/// Temporal Evolution to delete
#[gtest]
#[tokio::test]
async fn o_att_app_9009_a_write_after_the_history_was_deleted_records_a_new_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_recreated_id = namespace.urn("Building", "recreated")?;
    let building_type = namespace.entity_type("Building")?;
    let building_recreated = Entity::builder()
        .id(building_recreated_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Value::Number("21.5".parse()?))
        .vocab_property("category", VocabProperty::builder("office".parse::<VocabTerm>()?))
        .build()?;
    broker.entities().create(&building_recreated).send().await?.done()?;
    broker.temporal().delete(&building_recreated_id).send().await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_recreated_id)
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    let fragment = EntityFragment::builder().property("humidity", 44).build()?;
    broker.entities().append_attributes(&building_recreated_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_recreated_id)
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(evolution, has_evolution("humidity", all!(len(eq(1)), nth(0, any_attribute().value(44)))));
    expect_that!(evolution, lacks_evolution("temperature"));
    Ok(())
}
