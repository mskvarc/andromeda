//! The other instance keeps its instance id.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, instance_id_where},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Delete Attribute instance of the first of two instances of speed answers 204, and the Temporal Evolution
/// holds the second instance alone, under the `instanceId` it had before
/// Clause: 5.6.15, 4.5.7
#[gtest]
#[tokio::test]
async fn n_tmp_idl_9002_the_other_instance_keeps_its_instance_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_instance_deleted_id = namespace.urn("Vehicle", "instance-deleted")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_instance_deleted_evolution = TemporalEntity::builder()
        .id(vehicle_instance_deleted_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(1).observed_at("2021-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(2).observed_at("2021-01-02T00:00:00Z".parse()?))
        .build()?;
    broker
        .temporal()
        .create_or_update(&vehicle_instance_deleted_evolution)
        .send()
        .await?
        .created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_instance_deleted_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", len(eq(2))));
    let doomed = instance_id_where(evolution, &"speed".parse()?, any_attribute().value(1))?;
    let surviving = instance_id_where(evolution, &"speed".parse()?, any_attribute().value(2))?;
    broker
        .temporal()
        .delete_instance(&vehicle_instance_deleted_id, &"speed".parse::<AttributeName>()?, &doomed)
        .send()
        .await?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_instance_deleted_id)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(
        evolution_2,
        has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(2).instance_id(&surviving))))
    );
    Ok(())
}
