//! A backfill naming a new type adds it to current state.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    spelling_matcher::{listing, listing_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: a Create or Update Temporal Evolution naming Car for an existing Vehicle, its one instance observed before
/// the current one, leaves the current speed at 120 and gives the current Entity both types, so a Core query on Car
/// lists it
/// Clause: 4.5.5.3, 5.6.11.4, 4.3.1
/// Recorded: clause 5.6.11.4 adds the type to the Temporal Evolution and says nothing of current state; Andromeda
/// folds a Temporal API write into current state, and since Entity Type names carry no instant, clause 4.5.5.3
/// never makes a type the older, so the type is added even when no instance stands
#[gtest]
#[tokio::test]
async fn o_tmp_ups_9024_a_backfill_naming_a_new_type_adds_it_to_current_state() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let car_type = namespace.entity_type("Car")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2024-06-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_evolution_2 = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(car_type.clone())
        .property("speed", Property::builder(80).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution_2).send().await?.updated()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(&entity.entity_type, listing_count(2));
    expect_that!(&entity.entity_type, listing(&vehicle_type));
    expect_that!(&entity.entity_type, listing(&car_type));
    expect_that!(entity, has_attribute("speed", any_attribute().value(120)));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_id)));
    Ok(())
}
