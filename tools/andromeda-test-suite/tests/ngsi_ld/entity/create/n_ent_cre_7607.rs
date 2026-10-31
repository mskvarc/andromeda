//! An observed at with six fraction digits is kept as written.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: Create Entity whose Property carries an `observedAt` of six fractional-second digits, the clause 4.6.3
/// maximum, answers 201, and the Entity and its Temporal Evolution read the instant back with all six digits
/// Clause: 4.6.3, 4.6.4, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_7607_an_observed_at_with_six_fraction_digits_is_kept_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(1).observed_at("2026-03-04T05:06:07.123456Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute("speed", any_attribute().observed_at("2026-03-04T05:06:07.123456Z".parse()?))
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(len(eq(1)), nth(0, any_attribute().observed_at("2026-03-04T05:06:07.123456Z".parse()?)))
        )
    );
    Ok(())
}
