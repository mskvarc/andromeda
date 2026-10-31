//! A query relayed by the peer reaches the source with both aliases in via.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    context_source::ContextSource,
    forwarded_head,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    source_answer,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::{Accept, ReceivedBy},
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities on broker A by type, under an inclusive registration at broker B, which itself holds an
/// inclusive registration of a source, reaches the source with a Via naming broker-a and broker-b, and answers the
/// source Entity
/// Clause: 6.3.18, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_fed_qry_7607_a_query_relayed_by_the_peer_reaches_the_source_with_both_aliases_in_via() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let source_entities = [Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?];
    source
        .answer(Method::GET, "/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending("Vehicle", "1")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker_b.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker_b).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending("Vehicle", "1")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(page.items(), nth(0, has_id(&vehicle_id)));
    let forwarded = source.received_on(Method::GET, "/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(forwarded_head::via_hosts(forwarded_one), some(contains(eq(&"broker-a".parse::<ReceivedBy>()?))));
    expect_that!(forwarded_head::via_hosts(forwarded_one), some(contains(eq(&"broker-b".parse::<ReceivedBy>()?))));
    broker_b.registrations().delete(&registration_id).send().await?;
    Ok(())
}
