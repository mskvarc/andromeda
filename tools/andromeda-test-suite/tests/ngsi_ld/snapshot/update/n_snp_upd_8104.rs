//! A json null is bad request data and keeps the member.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotId},
};

/// Pins: Update Snapshot Status with endpoint null answers 400 `BadRequestData` naming endpoint, and the Snapshot
/// keeps its endpoint
/// Clause: 4.5.0, 5.2.1, 5.16.4.4
#[gtest]
#[tokio::test]
async fn n_snp_upd_8104_a_json_null_is_bad_request_data_and_keeps_the_member() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let endpoint = format!("{unreachable}/notify").parse::<url::Url>()?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .endpoint(endpoint.clone())
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    // A JSON null is no member change the fragment model can write, so it is sent as written.
    let refused = broker
        .send_unchecked(json_request_to(
            &broker,
            Method::PATCH,
            &format!("snapshots/{snapshot_id}"),
            r#"{"endpoint": null}"#,
        )?)
        .await;
    expect_that!(
        &refused,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("endpoint")
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await?;
    expect_that!(retrieved.endpoint(), some(eq(&endpoint)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
