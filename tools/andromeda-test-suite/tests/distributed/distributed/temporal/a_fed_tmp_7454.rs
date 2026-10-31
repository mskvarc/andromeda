//! A later value another holder keeps supersedes the hop target value in effect.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    linked_temporal_hop::{expect_hop_selects, register_peer, write_parking, write_vehicles},
    namespace::Namespace,
};
use googletest::prelude::*;

/// Pins: under `splitEntities` and `temporal_linked_retrieval` a between 12:00-13:00 query on broker E answers
/// `q=parkedIn{free}>0` for Vehicle 2 alone: broker E holds Parking 1 free 3 at 11:00 and broker B a later free 0 at
/// 11:30, broker E holds Parking 2 free 0 at 11:00 and broker B a later free 3 at 11:30; the value in effect when the
/// window opens is the latest across both holders
/// Clause: 5.7.4.4, 4.9, 4.11
/// Recorded: Andromeda extension, not v1.9.1: clause 5.7.4.4 raises `BadRequestData` when filter conditions
/// indicate the use of Linked Entity retrieval; the `temporal_linked_retrieval` extension, on broker E alone, admits
/// the hop and judges the Linked Entity on its instances valid within the window, the one each stream held when it
/// opened counted as clause 4.11 counts it for the entities the query returns
#[gtest]
#[tokio::test]
async fn a_fed_tmp_7454_a_later_value_another_holder_keeps_supersedes_the_hop_target_value_in_effect() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_e = broker::client(BrokerRole::E)?;
    let broker_b = broker::client(BrokerRole::B)?;
    write_parking(&broker_e, &namespace, 1, 3, "2020-08-01T11:00:00Z", None).await?;
    write_parking(&broker_b, &namespace, 1, 0, "2020-08-01T11:30:00Z", None).await?;
    write_parking(&broker_e, &namespace, 2, 0, "2020-08-01T11:00:00Z", None).await?;
    write_parking(&broker_b, &namespace, 2, 3, "2020-08-01T11:30:00Z", None).await?;
    write_vehicles(&broker_b, &broker_e, &namespace).await?;
    let registration = register_peer(&broker_e, &namespace, broker::peer(PeerRole::B)?).await?;
    let _registration_cleanup = Cleanup::of(&broker_e).registration_of(&registration);
    expect_hop_selects(&broker_e, &namespace, &namespace.urn("Vehicle", 2)?).await
}
