//! A purge after the history was deleted succeeds.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Value, VocabProperty, VocabTerm},
    query::Locality,
};

/// Pins: Purge Entities dropping temperature from an Entity whose Temporal Evolution was deleted first answers 204,
/// removing temperature and keeping the Entity and category
/// Clause: 5.6.16.4, 5.6.21.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, so an Entity created through the Core API has a
/// Temporal Evolution to delete
#[gtest]
#[tokio::test]
async fn o_ent_prg_9008_a_purge_after_the_history_was_deleted_succeeds() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_purge_id = namespace.urn("Building", "purge")?;
    let building_type = namespace.entity_type("Building")?;
    let building_purge = Entity::builder()
        .id(building_purge_id.clone())
        .entity_type(building_type)
        .property("temperature", Value::Number("21.5".parse()?))
        .vocab_property("category", VocabProperty::builder("office".parse::<VocabTerm>()?))
        .build()?;
    broker.entities().create(&building_purge).send().await?.done()?;
    broker.temporal().delete(&building_purge_id).send().await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_purge_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    broker
        .entities()
        .purge()
        .ids(vec![building_purge_id.clone()])
        .locality(Locality::Local)
        .drop(vec!["temperature".parse()?])
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_purge_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("temperature"));
    expect_that!(entity, has_attribute("category", any_attribute()));
    Ok(())
}
