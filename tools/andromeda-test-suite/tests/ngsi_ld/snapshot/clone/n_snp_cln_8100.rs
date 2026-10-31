//! Clone snapshot takes a snapshot body and applies its members.

use andromeda_test_suite::{
    broker,
    eventually::retried,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem},
    wire::{content_type, request_to},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_LENGTH};
use ngsi_ld::{
    http::headers::{Accept, Location},
    model::{APPLICATION_JSON, Duration, ErrorType, KeyValuePair, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotClone, SnapshotId, SnapshotPriority, SnapshotStatus},
};

/// Pins: Clone Snapshot without a body, with a type other than Snapshot, with `snapshotQueries` or with
/// `snapshotPriority` 11 answers 400; with id, `snapshotLifetime`, `snapshotPriority`, endpoint and `receiverInfo`
/// it answers 201 with an empty body and its Location, the clone carries those members and a `createdAt` not before
/// its source's, and a second clone under the same id answers 409 `AlreadyExists`; cloning a Snapshot that does not
/// exist answers 404 `ResourceNotFound`
/// Clause: 5.16.2.4, 6.38.3.1, Table 5.2.41-1
///
/// Invalid type: Entity (a body whose type names no Snapshot)
#[gtest]
#[tokio::test]
async fn n_snp_cln_8100_clone_snapshot_takes_a_snapshot_body_and_applies_its_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let source_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let clone_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
    let absent_id = format!("urn:ngsi-ld:Snapshot:{namespace}-absent").parse::<SnapshotId>()?;
    let third_id = format!("urn:ngsi-ld:Snapshot:{namespace}-3").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(source_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let source_created = retried(async || {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&source_id)
            .send()
            .await?;
        verify_that!(retrieved.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        Ok(retrieved.output().created_at)
    })
    .await?;
    let path = format!("snapshots/{source_id}/clone");
    // The refused clone bodies are no typed `SnapshotClone`, so each is written as text.
    let no_body = content_type(request_to(&broker, Method::POST, &path)?, &APPLICATION_JSON)?.append_header(CONTENT_LENGTH, HeaderValue::from_static("0"));
    let answer = broker.send_unchecked(no_body).await;
    expect_that!(&answer, answered(StatusCode::BAD_REQUEST));
    for body in [
        format!(r#"{{"id": "{clone_id}", "type": "Entity"}}"#),
        format!(r#"{{"id": "{clone_id}", "type": "Snapshot", "snapshotQueries": []}}"#),
        format!(r#"{{"id": "{clone_id}", "type": "Snapshot", "snapshotPriority": 11}}"#),
    ] {
        let request = json_request_to(&broker, Method::POST, &path, &body)?;
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    let missing = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&clone_id)
        .send()
        .await;
    expect_that!(&missing, answered(StatusCode::NOT_FOUND));
    let clone = SnapshotClone::builder()
        .id(clone_id.clone())
        .lifetime("PT2H".parse::<Duration>()?)
        .priority(6)
        .endpoint(sink.endpoint("clone")?.as_str())
        .receiver_info([KeyValuePair::new("X-Clone", "yes")])
        .build()?;
    let cloned = broker.with_responses().snapshots().clone_snapshot(&source_id, &clone).send().await?;
    expect_that!(
        cloned
            .head
            .typed::<Location>()
            .valid()
            .map(|location| location.as_iri_ref().as_str().to_owned()),
        some(eq(&format!("/ngsi-ld/v1/snapshots/{clone_id}")))
    );
    retried(async || {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&clone_id)
            .send()
            .await?;
        verify_that!(retrieved.status(), eq("success".parse::<SnapshotStatus>()?))?;
        verify_that!(retrieved.priority(), some(eq(SnapshotPriority::try_new(6)?)))?;
        verify_that!(retrieved.lifetime().map(ToString::to_string), some(eq("PT2H")))?;
        verify_that!(retrieved.endpoint(), some(eq(&sink.endpoint("clone")?)))?;
        verify_that!(retrieved.receiver_info().first().map(KeyValuePair::key), some(eq("X-Clone")))?;
        let origin_created = source_created.or_fail()?.timestamp();
        verify_that!(retrieved.output().created_at.map(|created| created.timestamp()), some(ge(origin_created)))?;
        Ok(())
    })
    .await?;
    let again = broker
        .with_responses()
        .snapshots()
        .clone_snapshot(&source_id, &SnapshotClone::builder().id(clone_id.clone()).build()?)
        .send()
        .await;
    expect_that!(&again, problem(StatusCode::CONFLICT, ErrorType::AlreadyExists));
    let unknown = broker
        .with_responses()
        .snapshots()
        .clone_snapshot(&absent_id, &SnapshotClone::builder().id(third_id).build()?)
        .send()
        .await;
    expect_that!(&unknown, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    broker.snapshots().delete(&clone_id).send().await?;
    broker.snapshots().delete(&source_id).send().await?;
    Ok(())
}
