//! An expired dataset instance is removed and the standing instances keep their expiry.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_instance, has_instances},
    eventually::retried,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: of three instances of one Property, the one whose `expiresAt` has passed is removed, while the one with a
/// future `expiresAt` keeps it and the one without an `expiresAt` stands
/// Clause: 4.22, 4.5.5, 6.3.11, 5.7.1.4
/// Recorded: 4.22 lets an expired attribute instance "be automatically removed" by a periodic clean-up whose lag
/// the spec leaves open, and does not refuse an `expiresAt` already past; Andromeda accepts it and removes the
/// instance, and the file waits up to 60 seconds for that
#[gtest]
#[tokio::test]
async fn o_ent_ret_0043_an_expired_dataset_instance_is_removed_and_the_standing_instances_keep_their_expiry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let dataset_a = namespace.dataset_id("Dataset", "a")?;
    let dataset_b = namespace.dataset_id("Dataset", "b")?;
    let dataset_c = namespace.dataset_id("Dataset", "c")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(80).dataset_id(dataset_a.clone()).expires_at("2020-01-01T00:00:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(78).dataset_id(dataset_b.clone()).expires_at("2099-12-31T23:59:59Z".parse()?),
        )
        .property("speed", Property::builder(75).dataset_id(dataset_c.clone()))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    retried(async || {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&vehicle_id)
            .system_attributes(SystemAttributes::Include)
            .send()
            .await?;
        let entity = retrieved.body();
        verify_that!(entity, has_instances("speed", 2))?;
        verify_that!(
            entity,
            has_instance(
                "speed",
                any_attribute().with_dataset_id(dataset_b.clone()).expires_at("2099-12-31T23:59:59Z".parse()?)
            )
        )?;
        verify_that!(entity, has_instance("speed", any_attribute().with_dataset_id(dataset_c.clone()).value(75)))?;
        verify_that!(entity, not(has_instance("speed", any_attribute().with_dataset_id(dataset_a.clone()))))?;
        Ok(())
    })
    .await?;
    Ok(())
}
