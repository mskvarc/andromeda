//! Delete all removes every instance.

use andromeda_test_suite::{
    broker,
    evolution_matcher::{has_evolution, lacks_evolution},
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::InstanceSelection},
    model::{AttributeName, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Delete Attribute with `deleteAll=true` answers 204 with no body and removes every instance of the
/// Attribute, whatever its dataset; the other Attributes stay
/// Clause: 5.6.13.4, 6.21.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_adl_0003_delete_all_removes_every_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(67).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-09-01T13:05:00Z".parse()?))
        .property(
            "fuelLevel",
            Property::builder(40)
                .dataset_id(namespace.dataset_id("Dataset", "fuel")?)
                .observed_at("2020-09-01T14:07:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let received = broker
        .with_responses()
        .temporal()
        .delete_attribute(&vehicle_id, &"fuelLevel".parse::<AttributeName>()?)
        .instances(InstanceSelection::All)
        .send()
        .await?;
    expect_that!(received.body.len(), eq(0));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", len(eq(2))));
    expect_that!(evolution, lacks_evolution("fuelLevel"));
    Ok(())
}
