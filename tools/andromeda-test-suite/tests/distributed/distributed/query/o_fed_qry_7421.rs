//! An entity only an auxiliary source answers counts as an entity.

use andromeda_test_suite::{
    bounded_federation::{hold_vehicles, register_peer},
    broker,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, model::ErrorType, registration::RegistrationMode};

/// Pins: Query Entities on broker-f, whose limits are 5 distinct entities and 15 source rows, over inclusive registrations at brokers B and C each holding Entities 1 to 5 and an auxiliary registration at broker D holding Entities 1 to 3, 9 and 10, 15 rows and 7 distinct identifiers, answers 403 `TooManyResults` whose detail names `federation.forwarding.max_query_entities`
/// Clause: 4.3.6.2, 4.12, 5.7.2.4
/// Recorded: clause 4.12 leaves the threshold of `TooManyResults` to the implementation; broker-f gathers at most 5 distinct entities and 15 source rows; an Entity only an auxiliary source answers is part of the merged result (clause 5.7.2.4), so it counts against the entity limit
#[gtest]
#[tokio::test]
async fn o_fed_qry_7421_an_entity_only_an_auxiliary_source_answers_counts_as_an_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_b = broker::client(BrokerRole::B)?;
    let broker_c = broker::client(BrokerRole::C)?;
    let broker_d = broker::client(BrokerRole::D)?;
    let bounded = broker::client(BrokerRole::F)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    hold_vehicles(&broker_b, &namespace, &[1, 2, 3, 4, 5]).await?;
    hold_vehicles(&broker_c, &namespace, &[1, 2, 3, 4, 5]).await?;
    hold_vehicles(&broker_d, &namespace, &[1, 2, 3, 9, 10]).await?;
    let registration_1 = register_peer(&bounded, &namespace, 1, PeerRole::B, RegistrationMode::Inclusive).await?;
    let registration_2 = register_peer(&bounded, &namespace, 2, PeerRole::C, RegistrationMode::Inclusive).await?;
    let registration_3 = register_peer(&bounded, &namespace, 3, PeerRole::D, RegistrationMode::Auxiliary).await?;
    let answer = bounded
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(
        &answer,
        problem(StatusCode::FORBIDDEN, ErrorType::TooManyResults).with_detail_containing("max_query_entities")
    );
    for registration_id in [&registration_1, &registration_2, &registration_3] {
        bounded.registrations().delete(registration_id).send().await?;
    }
    Ok(())
}
