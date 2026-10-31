//! Last n gives an attribute holding only instances without observed at no instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, lacks_evolution},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: an Entity whose speed holds 10 and 20 observed hourly and whose label holds "unobserved-1" and "unobserved-2"
/// with no observedAt answers lastN 1 with speed 20 and no label instance
/// Clause: 5.7.3.4, 4.5.7
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9061_last_n_gives_an_attribute_holding_only_instances_without_observed_at_no_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-01T01:00:00Z".parse()?))
        .property("label", "unobserved-1")
        .property("label", "unobserved-2")
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .last_n(1)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(&evolution.id, eq(&vehicle_id));
    expect_that!(evolution, has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(20)))));
    expect_that!(evolution, lacks_evolution("label"));
    Ok(())
}
