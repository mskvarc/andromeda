//! Last n over modified at keeps the latest modification of every holder without system attributes.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    cleanup::Cleanup,
    config::BrokerRole,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    peer_role::PeerRole,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Property, evolution::TemporalEntity},
    query::temporal_query::{self, TemporalProperty},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Temporal Evolution of Entities on broker A over `modifiedAt` with `lastN` 1 and no `sysAttrs`, broker
/// B holding speed 20 and broker A the speed 10 it wrote after, answers speed 10 alone, written without `createdAt`
/// and `modifiedAt`: the instants of every holder are compared before the response leaves them out
/// Clause: 5.7.4.4, 5.7.3.4, 4.5.5.3, 6.3.11, ETSI TS 104 175 V1.1.1 clause 8.5.3
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7465_last_n_over_modified_at_keeps_the_latest_modification_of_every_holder_without_system_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution_b = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(20).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker_b.temporal().create_or_update(&vehicle_evolution_b).send().await?.created()?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10).observed_at("2020-08-01T11:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query(
            temporal_query::after("2000-01-01T00:00:00Z".parse()?)
                .time_property(TemporalProperty::ModifiedAt)
                .last_n(1),
        )
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            has_evolution(
                "speed",
                all!(
                    len(eq(1)),
                    nth(0, any_attribute().value(10).without(Member::CreatedAt).without(Member::ModifiedAt))
                )
            )
        )
    );
    Ok(())
}
