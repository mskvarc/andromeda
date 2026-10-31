//! A repeated id merges against the state the earlier instance left.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property, Value},
};

/// Pins: a Batch Entity Merge naming one entity twice applies the second instance to the state the first one left:
/// a sub-attribute the first adds is updated by the second, an Attribute the first changes keeps its new value, and
/// the Attribute neither names stays
/// Clause: 5.5.11.0, 5.5.11.5, 5.6.20.4
#[gtest]
#[tokio::test]
async fn n_bat_mrg_0007_a_repeated_id_merges_against_the_state_the_earlier_instance_left() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", 20)
        .property("humidity", 40)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragments = [
        EntityFragment::for_entity(building_id.clone(), building_type.clone())
            .property("temperature", Property::builder(21).property("accuracy", Value::Number("0.5".parse()?)))
            .build()?,
        EntityFragment::for_entity(building_id.clone(), building_type.clone())
            .property("temperature", Property::builder(21).property("accuracy", Value::Number("0.25".parse()?)))
            .build()?,
    ];
    broker.entities().batch_merge(&fragments).send().await?.done()?;
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
            "temperature",
            any_attribute()
                .value(21)
                .sub_attribute("accuracy", any_attribute().value(Value::Number("0.25".parse()?)))
        )
    );
    expect_that!(entity, has_attribute("humidity", any_attribute().value(40)));
    Ok(())
}
