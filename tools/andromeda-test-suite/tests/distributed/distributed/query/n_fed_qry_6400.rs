//! A cascade appends each broker alias to the via chain.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    context_source::ContextSource,
    forwarded_head,
    namespace::Namespace,
    source_answer,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::{Accept, ReceivedBy},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities by type over one registration naming a mock source and one naming peer broker B, which
/// itself registers a mock source, reaches the first source with Via broker-a and the second with Via listing
/// broker-a and broker-b
/// Clause: 6.3.18, 4.3.6.4, 5.12
#[gtest]
#[tokio::test]
async fn n_fed_qry_6400_a_cascade_appends_each_broker_alias_to_the_via_chain() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let broker_b = broker::client(BrokerRole::B)?;
    let registration_3_id = namespace.registration_id(3)?;
    let source_entities = [];
    source
        .answer(Method::GET, "/direct/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let source_entities_2 = [];
    source
        .answer(Method::GET, "/chain/ngsi-ld/v1/entities", source_answer::entities(&source_entities_2))
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("direct")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let registration_3 = CSourceRegistration::builder(source.endpoint("chain")?)
        .id(registration_3_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker_b.registrations().register(&registration_3).send().await?;
    let _registration_3_cleanup = Cleanup::of(&broker_b).registration(&registration_3_id);
    broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    let forwarded = source.received_on(Method::GET, "/direct/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(forwarded_head::via_hosts(forwarded_one), some(contains(eq(&"broker-a".parse::<ReceivedBy>()?))));
    expect_that!(
        forwarded_head::via_hosts(forwarded_one),
        some(not(contains(eq(&"broker-b".parse::<ReceivedBy>()?))))
    );
    let forwarded_2 = source.received_on(Method::GET, "/chain/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded_2, len(eq(1)));
    let forwarded_one_2 = forwarded_2.first().or_fail()?;
    expect_that!(
        forwarded_head::via_hosts(forwarded_one_2),
        some(contains(eq(&"broker-a".parse::<ReceivedBy>()?)))
    );
    expect_that!(
        forwarded_head::via_hosts(forwarded_one_2),
        some(contains(eq(&"broker-b".parse::<ReceivedBy>()?)))
    );
    Ok(())
}
