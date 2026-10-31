//! A receiver info entry that is not an http header on an http endpoint is bad request data.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::problem,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, KeyValuePair, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotId},
};

/// Posts `body` as written to Create Snapshot and expects `BadRequestData` naming `needle` in the answer.
async fn expect_refused(broker: &BrokerClient, body: &str, needle: &str) -> Result<()> {
    // The temporal query breaks a rule the typed query refuses, so the body is written as text.
    let request = json_request_to(broker, Method::POST, "snapshots", body)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing(needle)
    );
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
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
}

/// Pins: Create Snapshot with an http endpoint whose `receiverInfo` has a key with a space or a value with a line
/// break answers 400 `BadRequestData` and stores nothing, while a good `receiverInfo` answers 201
/// Clause: 5.16.6, 5.2.41, 5.2.22
/// Recorded: the clauses require `receiverInfo` in the way required for the binding protocol; reading that as the
/// RFC 7230 header rule for an http endpoint is an inference
#[gtest]
#[tokio::test]
async fn n_snp_cre_8159_a_receiver_info_entry_that_is_not_an_http_header_on_an_http_endpoint_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let query = format!(r#"{{"type": "Query", "entities": [{{"type": "{vehicle_type}"}}]}}"#);
    let receivers = [("1", r#"{"key": "Bad Name", "value": "x"}"#), ("2", r#"{"key": "X-A", "value": "a\nb"}"#)];
    for (suffix, receiver) in receivers {
        let id = format!("urn:ngsi-ld:Snapshot:{namespace}-{suffix}").parse::<SnapshotId>()?;
        let endpoint = sink.endpoint(format!("s{suffix}"))?;
        // The receiver entries are no HTTP header, which the typed pair would carry as given, so the body is written as text.
        let body = format!(r#"{{"id": "{id}", "type": "Snapshot", "snapshotQueries": [{query}], "endpoint": "{endpoint}", "receiverInfo": [{receiver}]}}"#);
        expect_refused(&broker, &body, "").await?;
        expect_absent(&broker, &id).await;
    }
    let good_id = format!("urn:ngsi-ld:Snapshot:{namespace}-3").parse::<SnapshotId>()?;
    let good = Snapshot::builder()
        .id(good_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .endpoint(sink.endpoint("s3")?.as_str())
        .receiver_info([KeyValuePair::new("Authorization", "Bearer abc")])
        .build()?;
    broker.snapshots().create(&good).send().await?;
    broker.snapshots().delete(&good_id).send().await?;
    Ok(())
}
