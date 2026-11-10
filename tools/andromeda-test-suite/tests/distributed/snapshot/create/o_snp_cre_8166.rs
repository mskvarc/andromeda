//! A capture whose local and peer entities share one and merge to the entity limit succeeds.

use andromeda_test_suite::{
    bounded_federation::{hold_vehicles, register_peer},
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    eventually::retried,
    identity_matcher::has_id,
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::RegistrationMode,
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot on broker-f, whose limit is 5 distinct entities, over a type broker-f holds Entities 1 to 3 of
/// and peer broker B, under an inclusive registration, holds Entities 3 to 5 of, 6 Entities answered and 5 merged,
/// ends success, and Query Entities under it answers Entities 1 to 5
/// Clause: 4.12, 5.7.2.4, 5.16.1.4
/// Recorded: clause 4.12 leaves the threshold of `TooManyResults` to the implementation; broker-f gathers at most 5
/// distinct entities and 15 source rows; Entity 3 is one Entity of the merged collection whoever answers it (clause
/// 5.7.2.4), so the capture reaches the limit without passing it
#[gtest]
#[tokio::test]
async fn o_snp_cre_8166_a_capture_whose_local_and_peer_entities_share_one_and_merge_to_the_entity_limit_succeeds() -> Result<()> {
    let namespace = Namespace::fresh();
    let bounded = broker::client(BrokerRole::F)?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    hold_vehicles(&bounded, &namespace, &[1, 2, 3]).await?;
    hold_vehicles(&broker_b, &namespace, &[3, 4, 5]).await?;
    let registration_id = register_peer(&bounded, &namespace, 1, PeerRole::B, RegistrationMode::Inclusive).await?;
    let _registration_cleanup = Cleanup::of(&bounded).registration(&registration_id);
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    bounded.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let settled = bounded
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(settled.status(), eq("success".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    let page = bounded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(5)));
    for number in 1..=5 {
        expect_that!(page.items(), contains(has_id(&namespace.urn("Vehicle", number)?)));
    }
    bounded.snapshots().delete(&snapshot_id).send().await?;
    bounded.registrations().delete(&registration_id).send().await?;
    Ok(())
}
