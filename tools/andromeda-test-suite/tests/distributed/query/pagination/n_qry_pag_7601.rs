//! Count reports the local and peer total under any limit.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, ErrorType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities on broker A by type with count true, over two local Entities and one broker B serves
/// through an inclusive registration, answers NGSILD-Results-Count 3 with all three, with limit 1 one Entity, and
/// with limit 0 none; limit 0 without count answers 400 `BadRequestData`
/// Clause: 5.7.2.4, 5.5.9, 6.3.13
#[gtest]
#[tokio::test]
async fn n_qry_pag_7601_count_reports_the_local_and_peer_total_under_any_limit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle_2).send().await?.done()?;
    let vehicle_3 = Entity::builder()
        .id(vehicle_3_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    broker_b.entities().create(&vehicle_3).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending("Vehicle", "3")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(3)));
    expect_that!(page.items(), len(eq(3)));
    let page_2 = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .count(ResultsCount::Include)
        .limit(1)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(3)));
    expect_that!(page_2.items(), len(eq(1)));
    let page_3 = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .count(ResultsCount::Include)
        .limit(0)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(3)));
    expect_that!(page_3.items(), len(eq(0)));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::any()),
        Method::GET,
        &format!("entities?type=Vehicle_{namespace}&limit=0"),
    )?;
    let answer = broker.with_accept(Accept::any()).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
