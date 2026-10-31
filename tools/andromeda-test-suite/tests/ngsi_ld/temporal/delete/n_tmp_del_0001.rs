//! A deleted entity is created again with only its new history.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: after Delete Temporal Evolution of an Entity answers 204, posting the same id answers 201 and the entity
/// reads back with the new instance only
/// Clause: 5.6.16.4, 5.6.11.4, 6.19.3.2, 6.18.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_del_0001_a_deleted_entity_is_created_again_with_only_its_new_history() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(100).observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    broker.temporal().delete(&vehicle_id).send().await?;
    let vehicle_evolution_2 = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(200).observed_at("2020-09-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution_2).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(len(eq(1)), nth(0, any_attribute().value(200).observed_at("2020-09-01T12:03:00Z".parse()?)))
        )
    );
    Ok(())
}
