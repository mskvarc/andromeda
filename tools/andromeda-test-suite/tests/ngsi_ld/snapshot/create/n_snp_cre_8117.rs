//! A snapshot breaking its data type is bad request data.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    problem::{answered, problem},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    snapshot::SnapshotId,
};

/// Posts `body` as written to Create Snapshot and expects `BadRequestData`.
async fn expect_refused(broker: &BrokerClient, body: &str) -> Result<()> {
    // The bodies break the Snapshot data type, which the typed builders refuse, so each is written as text.
    let request = json_request_to(broker, Method::POST, "snapshots", body)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}

/// Asserts that no Snapshot `id` exists.
async fn expect_absent(broker: &BrokerClient, id: &SnapshotId) {
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
}

/// Pins: Create Snapshot answers 400 `BadRequestData` for a body without type, with a type other than Snapshot,
/// without any query, with a query lacking or misnaming its type, with an id that is not a URI and with
/// `snapshotQueries` that is not an array, and creates no Snapshot
/// Clause: 5.16.1.4, 5.2.41, 5.2.23
///
/// Invalid type: Entity (a body whose type names no Snapshot)
#[gtest]
#[tokio::test]
async fn n_snp_cre_8117_a_snapshot_breaking_its_data_type_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let query = format!(r#"{{"type": "Query", "entities": [{{"type": "{vehicle_type}"}}]}}"#);
    let id = snapshot_id.to_string();
    for body in [
        format!(r#"{{"id": "{id}", "snapshotQueries": [{query}]}}"#),
        format!(r#"{{"id": "{id}", "type": "Entity", "snapshotQueries": [{query}]}}"#),
        format!(r#"{{"id": "{id}", "type": "Snapshot"}}"#),
        format!(r#"{{"id": "{id}", "type": "Snapshot", "snapshotQueries": [{{"entities": [{{"type": "{vehicle_type}"}}]}}]}}"#),
        format!(r#"{{"id": "{id}", "type": "Snapshot", "snapshotQueries": [{{"type": "Entity"}}]}}"#),
        format!(r#"{{"id": "not a URI {namespace}", "type": "Snapshot", "snapshotQueries": [{query}]}}"#),
        format!(r#"{{"id": "{id}", "type": "Snapshot", "snapshotQueries": {query}}}"#),
    ] {
        expect_refused(&broker, &body).await?;
    }
    expect_absent(&broker, &snapshot_id).await;
    Ok(())
}
