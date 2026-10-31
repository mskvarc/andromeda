//! Sources sending more than the source row ceiling are too many results naming it.

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

/// Pins: Query Entities on broker-bounded, whose source row ceiling is 15, over four inclusive registrations at brokers B, C, D and E that each hold the same 5 Entities, 20 rows in all, answers 403 `TooManyResults` whose detail names `federation.forwarding.max_query_source_rows`
/// Clause: 4.12, 5.7.2.4
/// Recorded: clause 4.12 leaves the threshold of `TooManyResults` to the implementation; broker-bounded gathers at most 5 distinct entities and 15 source rows
#[gtest]
#[tokio::test]
async fn o_fed_qry_7419_sources_sending_more_than_the_source_row_ceiling_are_too_many_results_naming_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_b = broker::client(BrokerRole::B)?;
    let broker_c = broker::client(BrokerRole::C)?;
    let broker_d = broker::client(BrokerRole::D)?;
    let broker_e = broker::client(BrokerRole::E)?;
    let bounded = broker::client(BrokerRole::Bounded)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    for holder in [&broker_b, &broker_c, &broker_d, &broker_e] {
        hold_vehicles(holder, &namespace, &[1, 2, 3, 4, 5]).await?;
    }
    let registration_1 = register_peer(&bounded, &namespace, 1, PeerRole::B, RegistrationMode::Inclusive).await?;
    let registration_2 = register_peer(&bounded, &namespace, 2, PeerRole::C, RegistrationMode::Inclusive).await?;
    let registration_3 = register_peer(&bounded, &namespace, 3, PeerRole::D, RegistrationMode::Inclusive).await?;
    let registration_4 = register_peer(&bounded, &namespace, 4, PeerRole::E, RegistrationMode::Inclusive).await?;
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
        problem(StatusCode::FORBIDDEN, ErrorType::TooManyResults).with_detail_containing("max_query_source_rows")
    );
    for registration_id in [&registration_1, &registration_2, &registration_3, &registration_4] {
        bounded.registrations().delete(registration_id).send().await?;
    }
    Ok(())
}
