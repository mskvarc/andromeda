//! Three sources holding the same entities answer each once under the entity limit.

use andromeda_test_suite::{
    bounded_federation::{hold_vehicles, register_peer},
    broker,
    config::{BrokerRole, PeerRole},
    identity_matcher::has_id,
    namespace::Namespace,
    received_status::ExpectedStatus,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::EntityId,
    registration::RegistrationMode,
};

/// Pins: Query Entities on broker-f, whose limit is 5 distinct entities, over three inclusive registrations at brokers B, C and D that each hold the same 5 Entities, answers 200 with the 5 Entities once each, and a query with entityMap true answers 201 with a map whose pages of 2 cover the 5
/// Clause: 5.7.2.4, 4.3.6.7, 4.12, 5.5.14
#[gtest]
#[tokio::test]
async fn n_fed_qry_7418_three_sources_holding_the_same_entities_answer_each_once_under_the_entity_limit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_b = broker::client(BrokerRole::B)?;
    let broker_c = broker::client(BrokerRole::C)?;
    let broker_d = broker::client(BrokerRole::D)?;
    let bounded = broker::client(BrokerRole::F)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let numbers = [1, 2, 3, 4, 5];
    for holder in [&broker_b, &broker_c, &broker_d] {
        hold_vehicles(holder, &namespace, &numbers).await?;
    }
    let registration_1 = register_peer(&bounded, &namespace, 1, PeerRole::B, RegistrationMode::Inclusive).await?;
    let registration_2 = register_peer(&bounded, &namespace, 2, PeerRole::C, RegistrationMode::Inclusive).await?;
    let registration_3 = register_peer(&bounded, &namespace, 3, PeerRole::D, RegistrationMode::Inclusive).await?;
    let page = bounded.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(page.items(), len(eq(5)));
    for number in numbers {
        expect_that!(page.items(), contains(has_id(&namespace.urn("Vehicle", number)?)));
    }
    let entity_map_request = EntityMapRequest::returned();
    let mapped = bounded
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .entity_map(&entity_map_request)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map_header = mapped.entity_map().valid().or_fail()?;
    expect_that!(mapped.items(), len(eq(5)));
    let mut seen: Vec<EntityId> = Vec::new();
    for (offset, expected) in [(0, 2), (2, 2), (4, 1)] {
        let paged = bounded
            .with_heads()
            .with_accept(Accept::any())
            .entities()
            .query()
            .entity_type(&vehicle_type)
            .entity_map(&EntityMapRequest::reuse(map_header.clone()))
            .limit(2)
            .offset(offset)
            .send()
            .await?
            .expecting(StatusCode::OK)?;
        expect_that!(paged.items(), len(eq(expected)));
        for id in &seen {
            expect_that!(paged.items(), not(contains(has_id(id))));
        }
        seen.extend(paged.items().iter().map(|entity| entity.id.clone()));
    }
    for registration_id in [&registration_1, &registration_2, &registration_3] {
        bounded.registrations().delete(registration_id).send().await?;
    }
    Ok(())
}
