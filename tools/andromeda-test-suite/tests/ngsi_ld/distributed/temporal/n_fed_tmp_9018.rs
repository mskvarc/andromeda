//! An aggregated retrieve keeps only the source periods inside the window.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    forwarded_query::query_values,
    namespace::Namespace,
    nth::nth,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, evolution::AggregationMethod},
    query::temporal_query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Temporal Evolution aggregated per PT1H with `totalCount` between 2024-01-01T01:00 and 03:00, of an
/// Entity only an inclusive source holds, the source answering fuel periods 00:00-01:00, 01:00-02:00, 02:00-03:00 and
/// 02:30-03:30 and level only 00:00-01:00, answers 200 with fuel with the periods from 01:00 and 02:00 alone, and no
/// level; the source is asked once, for its aggregates
/// Clause: 4.11, 4.5.19, 5.7.3.4, 4.5.5
/// Recorded: the source is the entity's only holder, so its aggregate is the entity's and is kept as answered, its
/// periods narrowed to the window; an entity held in more than one part is aggregated over the instances of every
/// part instead
#[gtest]
#[tokio::test]
async fn n_fed_tmp_9018_an_aggregated_retrieve_keeps_only_the_source_periods_inside_the_window() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id(vehicle_id.clone())])
            .build()?])
        .operations([
            DistributedOperation::RetrieveEntity,
            DistributedOperation::QueryEntity,
            DistributedOperation::RetrieveTemporal,
            DistributedOperation::QueryTemporal,
        ])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    // The source answers periods the aggregated reader does not read as the model writes them, so the answer is written as text.
    source
        .answer_when(
            SourceRequest::matching(Method::GET, "/a/ngsi-ld/v1/temporal/entities/[^/]+"),
            source_answer::text(
                StatusCode::OK,
                HeaderValue::from_static("application/json"),
                &format!(
                    r#"{{"id": "urn:ngsi-ld:Vehicle:{namespace}-1", "type": "Vehicle_{namespace}", "fuel": {{"type": "Property", "totalCount": [[5, "2024-01-01T00:00:00Z", "2024-01-01T01:00:00Z"], [6, "2024-01-01T01:00:00Z", "2024-01-01T02:00:00Z"], [7, "2024-01-01T02:00:00Z", "2024-01-01T03:00:00Z"], [8, "2024-01-01T02:30:00Z", "2024-01-01T03:30:00Z"]]}}, "level": {{"type": "Property", "totalCount": [[9, "2024-01-01T00:00:00Z", "2024-01-01T01:00:00Z"]]}}}}"#
                ),
            ),
        )
        .await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&vehicle_id)
        .temporal_q(
            temporal_query::between("2024-01-01T01:00:00Z".parse()?, "2024-01-01T03:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::TotalCount])
                .aggregation_period("PT1H".parse()?),
        )
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate(
            "fuel",
            AggregationMethod::TotalCount,
            all!(
                len(eq(2)),
                nth(0, period().value(6).start("2024-01-01T01:00:00Z".parse()?)),
                nth(1, period().value(7).end("2024-01-01T03:00:00Z".parse()?))
            )
        )
    );
    expect_that!(aggregated, not(has_aggregate("level", AggregationMethod::TotalCount, anything())));
    let forwarded_request = SourceRequest::matching(Method::GET, "/a/ngsi-ld/v1/temporal/entities/[^/]+");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    expect_that!(query_values(forwarded.first().or_fail()?, "format"), elements_are![eq("aggregatedValues")]);
    Ok(())
}
