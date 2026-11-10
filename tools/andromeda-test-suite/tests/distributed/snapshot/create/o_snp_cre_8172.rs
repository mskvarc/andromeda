//! A temporal capture refused for its local and peer entities together keeps none of them.

use andromeda_test_suite::{
    bounded_federation::{hold_vehicle_histories, register_temporal_peer},
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    outcome::Done,
    snapshot_capture::{SnapshotQuery, await_query_problem},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::temporal_query::{self, TemporalQuery},
    registration::RegistrationMode,
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId},
};

/// Pins: A Snapshot on broker-f, whose limit is 5 distinct entities, with an entity query over a type broker-f holds
/// one Entity of and a temporal query over a type broker-f holds the histories of Entities 1 to 3 of and peer broker
/// B, under an inclusive `queryTemporal` registration, holds the histories of Entities 4 to 6 of, 3 Entities each and
/// 6 merged, ends partial, its temporal query's execution details are failure with a 403 `TooManyResults` problem whose
/// detail names `federation.query.max_entities`, and Query Temporal Evolution of Entities under the Snapshot answers
/// none of the 6
/// Clause: 4.12, 5.5.6, 5.7.4.4, 5.16.1.4, 5.2.42
/// Recorded: clause 5.5.6 leaves the threshold of `TooManyResults` to the implementation; broker-f gathers at most 5
/// distinct entities and 15 source rows; clause 5.16.1.4 executes a Snapshot temporal query following clause 5.7.4.4
/// and stores the Entity information it retrieved, so a query refused, as Query Temporal Evolution of Entities over the
/// same merged collection is, stores none of it
#[gtest]
#[tokio::test]
async fn o_snp_cre_8172_a_temporal_capture_refused_for_its_local_and_peer_entities_together_keeps_none_of_them() -> Result<()> {
    let namespace = Namespace::fresh();
    let bounded = broker::client(BrokerRole::F)?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let car_type = namespace.entity_type("Car")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let car = Entity::builder()
        .id(namespace.urn("Car", 1)?)
        .entity_type(car_type.clone())
        .property("speed", 1)
        .build()?;
    bounded.entities().create(&car).send().await?.done()?;
    hold_vehicle_histories(&bounded, &namespace, &[1, 2, 3]).await?;
    hold_vehicle_histories(&broker_b, &namespace, &[4, 5, 6]).await?;
    let registration_id = register_temporal_peer(&bounded, &namespace, 1, PeerRole::B, RegistrationMode::Inclusive).await?;
    let _registration_cleanup = Cleanup::of(&bounded).registration(&registration_id);
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([car_type.as_str()]).build()?)
        .temporal_query(
            QueryDocument::temporal_builder(TemporalQuery::after("2025-01-01T00:00:00Z".parse()?))
                .entities([vehicle_type.as_str()])
                .build()?,
        )
        .build()?;
    bounded.snapshots().create(&snapshot).send().await?;
    let problem = await_query_problem(&bounded, &snapshot_id, SettledStatus::Partial, SnapshotQuery::Temporal(0)).await?;
    expect_that!(problem.error_type, some(eq(&ErrorType::TooManyResults)));
    expect_that!(problem.status, some(eq(403)));
    expect_that!(problem.detail, some(contains_substring("federation.query.max_entities")));
    let captured = bounded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .temporal()
        .query(temporal_query::after("2025-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(captured.items(), is_empty());
    bounded.snapshots().delete(&snapshot_id).send().await?;
    bounded.registrations().delete(&registration_id).send().await?;
    Ok(())
}
