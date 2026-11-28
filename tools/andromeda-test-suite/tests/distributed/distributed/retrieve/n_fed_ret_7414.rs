//! An instance a source modified later wins the merge with its modified at.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    cleanup::Cleanup,
    config::BrokerRole,
    entity_matcher::{has_instance, has_instances},
    namespace::Namespace,
    outcome::Done,
    peer_role::PeerRole,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, Property},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Entity on broker A with options `sysAttrs`, A holding speed 10 without `observedAt`, merges the
/// speed 30 broker B wrote after it into speed 30 alone, carrying the `modifiedAt` broker B reports for it
/// Clause: 5.7.1.4, 4.5.5.3, ETSI TS 104 175 V1.1.1 clause 8.5.3, 6.3.11
#[gtest]
#[tokio::test]
async fn n_fed_ret_7414_an_instance_a_source_modified_later_wins_the_merge_with_its_modified_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_b = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(30))
        .build()?;
    broker_b.entities().create(&vehicle_b).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let held_by_b = broker_b
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let modified_at_b = system_time::of_attribute(held_by_b.body(), &["speed"], SystemMember::ModifiedAt)?;
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("speed", 1));
    expect_that!(entity, has_instance("speed", property().value(30)));
    expect_that!(system_time::of_attribute(entity, &["speed"], SystemMember::ModifiedAt), ok(eq(&modified_at_b)));
    Ok(())
}
