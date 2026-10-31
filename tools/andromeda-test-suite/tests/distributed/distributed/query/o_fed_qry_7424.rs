//! An entity an auxiliary source shares with another source counts once.

use andromeda_test_suite::{
    bounded_federation::{hold_vehicles, register_peer},
    broker,
    config::{BrokerRole, PeerRole},
    identity_matcher::has_id,
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, registration::RegistrationMode};

/// Pins: Query Entities on broker-f, whose limits are 5 distinct entities and 15 source rows, over inclusive registrations at brokers B and C each holding Entities 1 to 5 and an auxiliary registration at broker D holding Entities 1 to 3, 13 rows and 5 distinct identifiers, answers 200 with Entities 1 to 5
/// Clause: 4.12, 5.7.2.4
/// Recorded: clause 4.12 leaves the threshold of `TooManyResults` to the implementation; broker-f gathers at most 5 distinct entities and 15 source rows; an identifier an auxiliary source shares with another source is one entity of the merged result
#[gtest]
#[tokio::test]
async fn o_fed_qry_7424_an_entity_an_auxiliary_source_shares_with_another_source_counts_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_b = broker::client(BrokerRole::B)?;
    let broker_c = broker::client(BrokerRole::C)?;
    let broker_d = broker::client(BrokerRole::D)?;
    let bounded = broker::client(BrokerRole::F)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    hold_vehicles(&broker_b, &namespace, &[1, 2, 3, 4, 5]).await?;
    hold_vehicles(&broker_c, &namespace, &[1, 2, 3, 4, 5]).await?;
    hold_vehicles(&broker_d, &namespace, &[1, 2, 3]).await?;
    let registration_1 = register_peer(&bounded, &namespace, 1, PeerRole::B, RegistrationMode::Inclusive).await?;
    let registration_2 = register_peer(&bounded, &namespace, 2, PeerRole::C, RegistrationMode::Inclusive).await?;
    let registration_3 = register_peer(&bounded, &namespace, 3, PeerRole::D, RegistrationMode::Auxiliary).await?;
    let page = bounded.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(page.items(), len(eq(5)));
    for number in [1, 2, 3, 4, 5] {
        expect_that!(page.items(), contains(has_id(&namespace.urn("Vehicle", number)?)));
    }
    for registration_id in [&registration_1, &registration_2, &registration_3] {
        bounded.registrations().delete(registration_id).send().await?;
    }
    Ok(())
}
