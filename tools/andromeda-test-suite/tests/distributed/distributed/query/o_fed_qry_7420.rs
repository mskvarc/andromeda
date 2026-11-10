//! Sources answering more distinct entities than the limit are too many results naming it.

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

/// Pins: Query Entities on broker-f, whose limit is 5 distinct entities, over inclusive registrations at broker B holding Entities 1 to 5 and broker D holding Entities 1 to 6, 11 rows naming 6 distinct Entities, answers 403 `TooManyResults` whose detail names `federation.query.max_entities`
/// Clause: 4.12, 5.5.6, 5.7.2.4
/// Recorded: clause 5.5.6 leaves the threshold of `TooManyResults` to the implementation; broker-f gathers at most 5 distinct entities and 15 source rows
#[gtest]
#[tokio::test]
async fn o_fed_qry_7420_sources_answering_more_distinct_entities_than_the_limit_are_too_many_results_naming_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_b = broker::client(BrokerRole::B)?;
    let broker_d = broker::client(BrokerRole::D)?;
    let bounded = broker::client(BrokerRole::F)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    hold_vehicles(&broker_b, &namespace, &[1, 2, 3, 4, 5]).await?;
    hold_vehicles(&broker_d, &namespace, &[1, 2, 3, 4, 5, 6]).await?;
    let registration_1 = register_peer(&bounded, &namespace, 1, PeerRole::B, RegistrationMode::Inclusive).await?;
    let registration_2 = register_peer(&bounded, &namespace, 2, PeerRole::D, RegistrationMode::Inclusive).await?;
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
        problem(StatusCode::FORBIDDEN, ErrorType::TooManyResults).with_detail_containing("federation.query.max_entities")
    );
    for registration_id in [&registration_1, &registration_2] {
        bounded.registrations().delete(registration_id).send().await?;
    }
    Ok(())
}
