//! A backdated update is the current value and is recorded at its own instant.

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
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
    query::temporal_query,
};

/// Pins: Update Attributes of speed 10 observed at 2021-03-01 with speed 20 observed at 2021-01-01 answers 204; the
/// Entity holds 20, the window around 2021-01-01 holds the instance 20 alone and the window around 2021-03-01 the
/// instance 10 alone
/// Clause: 5.6.2, 4.3.1, 4.11, 5.7.3
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9018_a_backdated_update_is_the_current_value_and_is_recorded_at_its_own_instant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_backdated_id = namespace.urn("Vehicle", "backdated")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_backdated = Entity::builder()
        .id(vehicle_backdated_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(10).observed_at("2021-03-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_backdated).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("speed", Property::builder(20).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().update_attributes(&vehicle_backdated_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_backdated_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(20)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_backdated_id)
        .temporal_q(temporal_query::between("2020-12-31T00:00:00Z".parse()?, "2021-01-02T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(evolution, has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(20)))));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_backdated_id)
        .temporal_q(temporal_query::between("2021-02-28T00:00:00Z".parse()?, "2021-03-02T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution_2 = retrieved_3.body();
    expect_that!(evolution_2, has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(10)))));
    Ok(())
}
