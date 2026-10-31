//! An instance renders its observed at and no member of the broker s own.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Create Temporal Evolution of speed 55 observed at 2021-01-01 reads back the instance with `observedAt`
/// 2021-01-01T00:00:00Z and none of `recordedAt`, `recorded_at`, `writeId` or `writeOrdinal`
/// Clause: 4.5.2, 4.5.7, 5.2.21
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9040_an_instance_renders_its_observed_at_and_no_member_of_the_broker_s_own() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_members_id = namespace.urn("Vehicle", "members")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_members_evolution = TemporalEntity::builder()
        .id(vehicle_members_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(55).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_members_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_members_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                len(eq(1)),
                nth(
                    0,
                    any_attribute()
                        .observed_at("2021-01-01T00:00:00Z".parse()?)
                        .without_sub_attribute("recordedAt")
                        .without_sub_attribute("recorded_at")
                        .without_sub_attribute("writeId")
                        .without_sub_attribute("writeOrdinal")
                )
            )
        )
    );
    Ok(())
}
