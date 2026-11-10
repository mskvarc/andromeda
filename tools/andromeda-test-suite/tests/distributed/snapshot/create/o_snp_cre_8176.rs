//! A capture refused after it stored pages keeps none of them.

use andromeda_test_suite::{
    bounded_federation::{hold_vehicles, register_peer},
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    snapshot_capture::{SnapshotQuery, await_query_problem},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    registration::RegistrationMode,
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId},
};

/// Pins: A Snapshot on broker-f, whose limit is 5 distinct entities and which captures two entities per page, with an
/// entity query over a type broker-f holds one Entity of and an entity query over a type broker-f holds Entities 1 to
/// 6 of and peer broker B, under an inclusive registration, holds Entity 7 of, 7 merged, ends partial, its second
/// query's execution details are failure with a 403 `TooManyResults` problem whose detail names
/// `federation.query.max_entities`, and Query Entities under the Snapshot answers the first query's Entity and none of
/// the 7
/// Clause: 4.12, 5.7.2.4, 5.16.1.4, 5.2.42
/// Recorded: clause 4.12 leaves the threshold of `TooManyResults` to the implementation; broker-f gathers at most 5
/// distinct entities and 15 source rows; clause 5.16.1.4 executes a Snapshot query following clause 5.7.2.4 and stores
/// the Entity information it retrieved, and a query refused as Query Entities over the same merged collection is
/// retrieves none, however many of its pages the capture read before the merged collection passed the limit
#[gtest]
#[tokio::test]
async fn o_snp_cre_8176_a_capture_refused_after_it_stored_pages_keeps_none_of_them() -> Result<()> {
    let namespace = Namespace::fresh();
    let bounded = broker::client(BrokerRole::F)?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let car_type = namespace.entity_type("Car")?;
    let car_id = namespace.urn("Car", 1)?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let car = Entity::builder()
        .id(car_id.clone())
        .entity_type(car_type.clone())
        .property("speed", 1)
        .build()?;
    bounded.entities().create(&car).send().await?.done()?;
    hold_vehicles(&bounded, &namespace, &[1, 2, 3, 4, 5, 6]).await?;
    hold_vehicles(&broker_b, &namespace, &[7]).await?;
    let registration_id = register_peer(&bounded, &namespace, 1, PeerRole::B, RegistrationMode::Inclusive).await?;
    let _registration_cleanup = Cleanup::of(&bounded).registration(&registration_id);
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([car_type.as_str()]).build()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    bounded.snapshots().create(&snapshot).send().await?;
    let problem = await_query_problem(&bounded, &snapshot_id, SettledStatus::Partial, SnapshotQuery::Entity(1)).await?;
    expect_that!(problem.error_type, some(eq(&ErrorType::TooManyResults)));
    expect_that!(problem.status, some(eq(403)));
    expect_that!(problem.detail, some(contains_substring("federation.query.max_entities")));
    let reader = bounded.with_accept(Accept::new(NgsiLdMediaType::Json));
    let captured = reader.on_snapshot(&snapshot_id);
    let vehicles = captured.entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(vehicles.items(), is_empty());
    let cars = captured.entities().query().entity_type(&car_type).send().await?;
    expect_that!(cars.items(), elements_are![has_id(&car_id)]);
    bounded.snapshots().delete(&snapshot_id).send().await?;
    bounded.registrations().delete(&registration_id).send().await?;
    Ok(())
}
