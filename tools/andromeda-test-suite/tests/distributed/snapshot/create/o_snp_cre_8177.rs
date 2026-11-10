//! A temporal capture refused on a resumed pass keeps none of its history.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    cleanup::Cleanup,
    config::BrokerRole,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    snapshot_capture::{SnapshotQuery, await_query_problem},
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_RANGE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, Input, Lossless, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query::{self, TemporalQuery},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId},
};
use wiremock::ResponseTemplate;

/// The path the source answers Query Temporal Evolution of Entities on, beneath its root `a`.
const TEMPORAL_PATH: &str = "/a/ngsi-ld/v1/temporal/entities";

/// The history of the Vehicle `number` of the namespace, one `speed` instance of `value` observed at `observed_at`.
fn history(namespace: &Namespace, number: u32, value: u32, observed_at: &str) -> Result<TemporalEntity<Input>> {
    Ok(TemporalEntity::builder()
        .id(namespace.urn("Vehicle", number)?)
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("speed", Property::builder(value).observed_at(observed_at.parse()?))
        .build()?)
}

/// Pins: A Snapshot on broker-f, whose limit is 5 distinct entities and which captures two entities per page, with an
/// entity query over a type broker-f holds Entity 1 of, `speed` 1 and no observation, and a temporal query after
/// 2025-01-01 over that type, whose `queryTemporal` source answers the window with a 206 holding Entity 1 observed at
/// 2026-01-01 and a `Content-Range` ending there, and the window resumed there with the histories of Entities 2 to 7,
/// ends partial, its temporal query's execution details are failure with a 403 `TooManyResults` problem whose detail
/// names `federation.query.max_entities`, Query Temporal Evolution of Entities under the Snapshot answers no history,
/// and Query Entities under it answers Entity 1 with `speed` 1
/// Clause: 4.12, 5.7.4.4, 5.16.1.4, 5.2.42, 6.3.10
/// Recorded: clause 4.12 leaves the threshold of `TooManyResults` to the implementation; broker-f gathers at most 5
/// distinct entities and 15 source rows; clause 5.16.1.4 retrieves every page of a temporal query, the rest of a
/// partial answer asked for as clause 6.3.10 says, and stores the Entity information it retrieved, which a query
/// refused on the rest of its answer has none of; the entity query captured Entity 1 whatever the temporal query wrote
#[gtest]
#[tokio::test]
async fn o_snp_cre_8177_a_temporal_capture_refused_on_a_resumed_pass_keeps_none_of_its_history() -> Result<()> {
    let namespace = Namespace::fresh();
    let bounded = broker::client(BrokerRole::F)?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let registration_id = namespace.registration_id(1)?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    bounded.entities().create(&vehicle).send().await?.done()?;
    let cut = history(&namespace, 1, 100, "2026-01-01T00:00:00Z")?;
    let resumed = (2..=7)
        .map(|number| history(&namespace, number, number, "2026-01-02T00:00:00Z"))
        .collect::<Result<Vec<_>>>()?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, TEMPORAL_PATH).query("offset", 2).priority(1),
            source_answer::temporal_entities(&[]),
        )
        .await?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, TEMPORAL_PATH)
                .query_containing("timeAt", "2025-01-01T")
                .priority(2),
            ResponseTemplate::new(StatusCode::PARTIAL_CONTENT.as_u16())
                .set_body_raw(format!("[{}]", cut.render(Lossless::Normalized)), "application/json")
                .insert_header(CONTENT_RANGE, HeaderValue::from_static("DateTime 2025-01-01T00:00:00Z-2026-01-01T00:00:00Z/*")),
        )
        .await?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, TEMPORAL_PATH)
                .query_containing("timeAt", "2026-01-01T")
                .priority(3),
            source_answer::temporal_entities(&resumed),
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
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
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
    let reader = bounded.with_accept(Accept::new(NgsiLdMediaType::Json));
    let captured = reader.on_snapshot(&snapshot_id);
    let histories = captured
        .temporal()
        .query(temporal_query::after("2025-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(histories.items(), is_empty());
    let vehicles = captured.entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(
        vehicles.items(),
        elements_are![all!(has_id(&vehicle_id), has_attribute("speed", property().value(1)))]
    );
    bounded.snapshots().delete(&snapshot_id).send().await?;
    bounded.registrations().delete(&registration_id).send().await?;
    Ok(())
}
