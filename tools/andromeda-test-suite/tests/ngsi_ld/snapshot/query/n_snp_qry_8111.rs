//! The snapshot header is echoed on a success and on an error.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::headers::{Accept, NgsiLdSnapshot},
    model::{ErrorType, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: An operation with NGSILD-Snapshot naming a Snapshot answers with the same header both when it succeeds
/// (Query Entities, 200) and when it fails (Delete Entity of an Entity the Snapshot does not hold, 404
/// `ResourceNotFound`)
/// Clause: 6.3.22, 5.6.6.4
#[gtest]
#[tokio::test]
async fn n_snp_qry_8111_the_snapshot_header_is_echoed_on_a_success_and_on_an_error() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_absent_id = namespace.urn("Vehicle", "absent")?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let snapshot_2 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(snapshot_2.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    let page = headed.outcome;
    expect_that!(
        headed.head.typed::<NgsiLdSnapshot>().valid().map(NgsiLdSnapshot::snapshot),
        some(eq(&snapshot_id))
    );
    expect_that!(page.items(), len(eq(0)));
    let answer = broker
        .with_responses()
        .on_snapshot(&snapshot_id)
        .entities()
        .delete(&vehicle_absent_id)
        .send()
        .await;
    expect_that!(
        &answer,
        problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound)
            .with_header(HeaderName::from_static("ngsild-snapshot"), &format!("urn:ngsi-ld:Snapshot:{namespace}-1"))
    );
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
