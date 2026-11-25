//! A hop target is judged on what every holder wrote inside the window.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::BrokerRole,
    linked_temporal_hop::{expect_hop_selects, register_peer, write_parking, write_vehicles},
    namespace::Namespace,
    peer_role::PeerRole,
};
use googletest::prelude::*;

/// Pins: under `splitEntities` and `temporal_linked_retrieval` a between 12:00-13:00 query on broker E answers
/// `q=parkedIn{free}>0` for Vehicle 1 alone: broker E holds Parking 1 free 3 at 12:20 and broker B a later free 0 at
/// 12:40, broker E holds Parking 2 free 0 at 12:20 and broker B a later free 0 at 12:40; the hop is existential over
/// the instances inside the window of every holder, so a later zero does not hide an earlier three
/// Clause: 5.7.4.4, 4.9, 4.11
/// Recorded: Andromeda extension, not v1.9.1: clause 5.7.4.4 raises `BadRequestData` when filter conditions
/// indicate the use of Linked Entity retrieval; the `temporal_linked_retrieval` extension, on broker E alone, admits
/// the hop and judges the Linked Entity on its instances inside the window, as clause 5.7.4.4 steps S2 and S3 judge
/// the entities the query returns
#[gtest]
#[tokio::test]
async fn a_fed_tmp_7454_a_hop_target_is_judged_on_what_every_holder_wrote_inside_the_window() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_e = broker::client(BrokerRole::E)?;
    let broker_b = broker::client(BrokerRole::B)?;
    write_parking(&broker_e, &namespace, 1, 3, "2020-08-01T12:20:00Z", None).await?;
    write_parking(&broker_b, &namespace, 1, 0, "2020-08-01T12:40:00Z", None).await?;
    write_parking(&broker_e, &namespace, 2, 0, "2020-08-01T12:20:00Z", None).await?;
    write_parking(&broker_b, &namespace, 2, 0, "2020-08-01T12:40:00Z", None).await?;
    write_vehicles(&broker_b, &broker_e, &namespace).await?;
    let registration = register_peer(&broker_e, &namespace, broker::peer(PeerRole::B)?).await?;
    let _registration_cleanup = Cleanup::of(&broker_e).registration_of(&registration);
    expect_hop_selects(&broker_e, &namespace, &namespace.urn("Vehicle", 1)?).await
}
