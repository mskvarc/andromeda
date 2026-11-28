//! An instance a source modified later wins the merge of a query without system attributes.

use andromeda_test_suite::{
    attribute_matcher::property,
    attribute_member::Member,
    broker,
    cleanup::Cleanup,
    config::BrokerRole,
    entity_matcher::{has_instance, has_instances},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    peer_role::PeerRole,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Property},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Entities by type on broker A, which holds speed 10 without `observedAt`, answers the Vehicle with the
/// speed 30 broker B wrote after it alone, written without `createdAt` and `modifiedAt` because the request did not
/// ask for `sysAttrs`
/// Clause: 5.7.2.4, 4.5.5.3, ETSI TS 104 175 V1.1.1 clause 8.5.3, 6.3.11
#[gtest]
#[tokio::test]
async fn n_fed_qry_7425_an_instance_a_source_modified_later_wins_the_merge_of_a_query_without_system_attributes() -> Result<()> {
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
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_instances("speed", 1)));
    expect_that!(
        page.items(),
        nth(
            0,
            has_instance("speed", property().value(30).without(Member::CreatedAt).without(Member::ModifiedAt))
        )
    );
    Ok(())
}
