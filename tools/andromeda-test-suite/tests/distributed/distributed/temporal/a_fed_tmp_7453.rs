//! A hop target written only before the window is judged on its value in effect.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    linked_temporal_hop::{expect_hop_selects, register_peer, write_parking, write_vehicles},
    namespace::Namespace,
};
use googletest::prelude::*;

/// Pins: under `splitEntities` and `temporal_linked_retrieval` a between 12:00-13:00 query on broker E answers
/// `q=parkedIn{free}>0` for Vehicle 1, whose Parking 1 broker B last wrote free 3 at 11:00, and not for Vehicle 2,
/// whose Parking 2 broker B last wrote free 0 at 11:00; neither Parking holds an instance inside the window, so each
/// hop reads the value in effect when the window opens
/// Clause: 5.7.4.4, 4.9, 4.11
/// Recorded: Andromeda extension, not v1.9.1: clause 5.7.4.4 raises `BadRequestData` when filter conditions
/// indicate the use of Linked Entity retrieval; the `temporal_linked_retrieval` extension, on broker E alone, admits
/// the hop and judges the Linked Entity on its instances valid within the window, the one each stream held when it
/// opened counted as clause 4.11 counts it for the entities the query returns
#[gtest]
#[tokio::test]
async fn a_fed_tmp_7453_a_hop_target_written_only_before_the_window_is_judged_on_its_value_in_effect() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_e = broker::client(BrokerRole::E)?;
    let broker_b = broker::client(BrokerRole::B)?;
    write_parking(&broker_b, &namespace, 1, 3, "2020-08-01T11:00:00Z", None).await?;
    write_parking(&broker_b, &namespace, 2, 0, "2020-08-01T11:00:00Z", None).await?;
    write_vehicles(&broker_b, &broker_e, &namespace).await?;
    let registration = register_peer(&broker_e, &namespace, broker::peer(PeerRole::B)?).await?;
    let _registration_cleanup = Cleanup::of(&broker_e).registration_of(&registration);
    expect_hop_selects(&broker_e, &namespace, &namespace.urn("Vehicle", 1)?).await
}
