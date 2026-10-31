//! An attribute changed twice has three instances latest first under last n.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, instance_id_at},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
    query::temporal_query,
};

/// Pins: after Create Entity of a Sensor with temperature 20 and two Update Attributes setting 21 and 22, each at a
/// later `observedAt`, Retrieve Temporal Evolution of an Entity answers three temperature instances with three
/// distinct `instanceIds` in `observedAt` order, and with `lastN=1` the latest alone, 22
/// Clause: 4.3.1, 4.5.7, 5.7.3, 5.7.3.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9030_an_attribute_changed_twice_has_three_instances_latest_first_under_last_n() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_thrice_id = namespace.urn("Sensor", "thrice")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_thrice = Entity::builder()
        .id(sensor_thrice_id.clone())
        .entity_type(sensor_type)
        .property("temperature", Property::builder(20).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&sensor_thrice).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("temperature", Property::builder(21).observed_at("2024-01-02T00:00:00Z".parse()?))
        .build()?;
    broker.entities().update_attributes(&sensor_thrice_id, &fragment).send().await?.done()?;
    let fragment_2 = EntityFragment::builder()
        .property("temperature", Property::builder(22).observed_at("2024-01-03T00:00:00Z".parse()?))
        .build()?;
    broker.entities().update_attributes(&sensor_thrice_id, &fragment_2).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = json
        .temporal()
        .retrieve(&sensor_thrice_id)
        .temporal_q(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "temperature",
            all!(
                len(eq(3)),
                nth(0, any_attribute().value(20)),
                nth(1, any_attribute().value(21)),
                nth(2, any_attribute().value(22).observed_at("2024-01-03T00:00:00Z".parse()?))
            )
        )
    );
    let first = instance_id_at(evolution, &"temperature".parse()?, 0)?;
    let second = instance_id_at(evolution, &"temperature".parse()?, 1)?;
    let third = instance_id_at(evolution, &"temperature".parse()?, 2)?;
    let latest = json
        .temporal()
        .retrieve(&sensor_thrice_id)
        .temporal_q(temporal_query::after("2023-01-01T00:00:00Z".parse()?).last_n(1))
        .send()
        .await?;
    expect_that!(
        latest.body(),
        has_evolution("temperature", all!(len(eq(1)), nth(0, any_attribute().value(22).instance_id(&third))))
    );
    expect_that!(third == second, eq(false));
    expect_that!(third == first, eq(false));
    Ok(())
}
