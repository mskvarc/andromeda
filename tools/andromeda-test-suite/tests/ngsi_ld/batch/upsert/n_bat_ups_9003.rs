//! Every instance of a repeated id reaches the temporal evolution.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: Batch Entity Upsert naming one new entity twice with `airQualityLevel` 1 and then 2 observed at different
/// instants answers 201, and its Temporal Evolution holds both observations
/// Clause: 5.5.11.2, 5.5.11.0, 5.6.8.4, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_bat_ups_9003_every_instance_of_a_repeated_id_reaches_the_temporal_evolution() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("airQualityLevel", Property::builder(1).observed_at("2020-08-01T12:03:00Z".parse()?))
            .build()?,
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("airQualityLevel", Property::builder(2).observed_at("2020-08-01T12:05:00Z".parse()?))
            .build()?,
    ];
    broker.entities().batch_upsert(&entities).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "airQualityLevel",
            all!(len(eq(2)), contains(any_attribute().value(1)), contains(any_attribute().value(2)))
        )
    );
    Ok(())
}
