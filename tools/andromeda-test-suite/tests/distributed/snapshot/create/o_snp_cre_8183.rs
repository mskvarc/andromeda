//! A temporal capture reads a source's backward pages and keeps the last n instances.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::BrokerRole,
    context_source::ContextSource,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    snapshot_capture::{SnapshotQuery, await_query_result},
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_RANGE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Input, Lossless, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
    snapshot::{ExecutionStatus, QueryDocument, SettledStatus, Snapshot, SnapshotId},
};
use wiremock::ResponseTemplate;

/// The path the source answers Query Temporal Evolution of Entities on, beneath its root `a`.
const TEMPORAL_PATH: &str = "/a/ngsi-ld/v1/temporal/entities";

/// The history of Vehicle 1 of the namespace, a `speed` instance of each `day` observed on that day of January 2026.
fn history(namespace: &Namespace, days: &[u32]) -> Result<TemporalEntity<Input>> {
    let builder = TemporalEntity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type(namespace.entity_type("Vehicle")?);
    let builder = days.iter().try_fold(builder, |builder, day| -> Result<_> {
        Ok(builder.property("speed", Property::builder(*day).observed_at(format!("2026-01-{day:02}T00:00:00Z").parse()?)))
    })?;
    Ok(builder.build()?)
}

/// A 206 holding the history of `days`, cut backwards at the range `range` (clause 6.3.10).
fn backward_page(namespace: &Namespace, days: &[u32], range: &'static str) -> Result<ResponseTemplate> {
    Ok(ResponseTemplate::new(StatusCode::PARTIAL_CONTENT.as_u16())
        .set_body_raw(format!("[{}]", history(namespace, days)?.render(Lossless::Normalized)), "application/json")
        .insert_header(CONTENT_RANGE, HeaderValue::from_static(range)))
}

/// Pins: A Snapshot on broker-f, which captures two entities per page, with a temporal query after 2025-01-01 with
/// `lastN` 3 over a type broker-f holds no Entity of, whose `queryTemporal` source answers the window with a 206 holding
/// Vehicle 1's `speed` of January 9 and 10 2026 and a `Content-Range` from the 10th to the 9th of size 3, the window
/// ending on the 9th with a 206 holding the 7th and 8th and a range from the 8th to the 7th, and the window ending on
/// the 7th with the 5th and 6th, ends success; its temporal query's execution details are success with no
/// `problemDetails`, the source is asked `lastN` 3 over the windows ending on the 9th and on the 7th, and Query
/// Temporal Evolution of Entities under the Snapshot with `lastN` 3 answers Vehicle 1 with `speed` 8, 9 and 10
/// Clause: Table 5.2.21-1, 4.11, 5.16.1.4, 5.7.4.4, 6.3.10
/// Recorded: clause 6.3.10 pages a `lastN` backwards and ends a backward range at the least recent instant of its
/// page, but defines no request that continues it; Andromeda asks the same query with the same `lastN` over the window
/// ending at that instant, which a closing instant leaves out (clause 4.11), and keeps of each attribute only the most
/// recent instances it still lacks to reach `lastN`
#[gtest]
#[tokio::test]
async fn o_snp_cre_8183_a_temporal_capture_reads_a_sources_backward_pages_and_keeps_the_last_n_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let bounded = broker::client(BrokerRole::F)?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let registration_id = namespace.registration_id(1)?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, TEMPORAL_PATH).query("offset", 2).priority(1),
            source_answer::temporal_entities(&[]),
        )
        .await?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, TEMPORAL_PATH).query("timerel", "after").priority(2),
            backward_page(&namespace, &[9, 10], "DateTime 2026-01-10T00:00:00Z-2026-01-09T00:00:00Z/3")?,
        )
        .await?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, TEMPORAL_PATH)
                .query_containing("endTimeAt", "2026-01-09T")
                .priority(3),
            backward_page(&namespace, &[7, 8], "DateTime 2026-01-08T00:00:00Z-2026-01-07T00:00:00Z/3")?,
        )
        .await?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, TEMPORAL_PATH)
                .query_containing("endTimeAt", "2026-01-07T")
                .priority(4),
            source_answer::temporal_entities(&[history(&namespace, &[5, 6])?]),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    bounded.registrations().register(&registration).send().await?;
    let _registration_cleanup = Cleanup::of(&bounded).registration(&registration_id);
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .temporal_query(
            QueryDocument::temporal_builder(temporal_query::after("2025-01-01T00:00:00Z".parse()?).last_n(3).build()?)
                .entities([vehicle_type.as_str()])
                .build()?,
        )
        .build()?;
    bounded.snapshots().create(&snapshot).send().await?;
    let detail = await_query_result(&bounded, &snapshot_id, SettledStatus::Success, SnapshotQuery::Temporal(0)).await?;
    expect_that!(detail.result_status, eq(ExecutionStatus::Success));
    expect_that!(detail.problem_details, none());
    for closing in ["2026-01-09T", "2026-01-07T"] {
        let resumed = source
            .received_matching(
                &SourceRequest::new(Method::GET, TEMPORAL_PATH)
                    .query("timerel", "between")
                    .query("lastN", 3)
                    .query_containing("endTimeAt", closing),
            )
            .await?;
        expect_that!(resumed, not(is_empty()), "the window ending at {closing}");
    }
    let histories = bounded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .temporal()
        .query(temporal_query::after("2025-01-01T00:00:00Z".parse()?).last_n(3))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(
        histories.items(),
        elements_are![all!(
            has_id(&vehicle_id),
            has_evolution(
                "speed",
                all!(
                    len(eq(3)),
                    nth(0, any_attribute().value(8)),
                    nth(1, any_attribute().value(9)),
                    nth(2, any_attribute().value(10))
                )
            )
        )]
    );
    bounded.snapshots().delete(&snapshot_id).send().await?;
    bounded.registrations().delete(&registration_id).send().await?;
    Ok(())
}
