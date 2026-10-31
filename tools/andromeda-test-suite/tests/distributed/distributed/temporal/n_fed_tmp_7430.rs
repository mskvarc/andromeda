//! The source history is read past its page cut.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    nth::nth,
};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode, header::CONTENT_RANGE};
use ngsi_ld::{
    http::headers::Accept,
    model::{
        InstanceId,
        Lossless,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::{EntitySplitting, q, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};
use wiremock::{
    Mock,
    ResponseTemplate,
    matchers::{method, path},
};

/// Pins: Query Temporal Evolution of Entities as `aggregatedValues` `totalCount` over a source answering 206 with a
/// Content-Range for the first two of four instances counts all four instances
/// Clause: 5.7.4.4, 6.3.10, 4.5.19
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7430_the_source_history_is_read_past_its_page_cut() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let first_cut = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .identified(
            "speed",
            namespace.urn_text("instance", 1).parse::<InstanceId>()?,
            Property::builder(1).observed_at("2020-08-01T12:00:00Z".parse()?),
        )
        .identified(
            "speed",
            namespace.urn_text("instance", 2).parse::<InstanceId>()?,
            Property::builder(2).observed_at("2020-08-01T13:00:00Z".parse()?),
        )
        .build()?;
    let rest = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .identified(
            "speed",
            namespace.urn_text("instance", 2).parse::<InstanceId>()?,
            Property::builder(2).observed_at("2020-08-01T13:00:00Z".parse()?),
        )
        .identified(
            "speed",
            namespace.urn_text("instance", 3).parse::<InstanceId>()?,
            Property::builder(3).observed_at("2020-08-01T14:00:00Z".parse()?),
        )
        .identified(
            "speed",
            namespace.urn_text("instance", 4).parse::<InstanceId>()?,
            Property::builder(4).observed_at("2020-08-01T15:00:00Z".parse()?),
        )
        .build()?;
    // A source answers a temporal query it cuts with 206 and a Content-Range (clause 6.3.10), which the source answer
    // helpers do not write, so both answers are built here from the typed evolutions.
    let cut_answer = ResponseTemplate::new(StatusCode::PARTIAL_CONTENT.as_u16())
        .set_body_raw(format!("[{}]", first_cut.render(Lossless::Normalized)), "application/json")
        .insert_header(CONTENT_RANGE, HeaderValue::from_static("DateTime 2020-08-01T12:00:00Z-2020-08-01T13:00:00Z/4"));
    let rest_answer = ResponseTemplate::new(StatusCode::OK.as_u16()).set_body_raw(format!("[{}]", rest.render(Lossless::Normalized)), "application/json");
    source
        .answer_in_turn(
            || Mock::given(method("GET")).and(path("/a/ngsi-ld/v1/temporal/entities")),
            vec![cut_answer, rest_answer],
        )
        .await?;
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query_aggregated(temporal_query::before("2030-01-01T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Avg, AggregationMethod::TotalCount]))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .q(q::attr("speed").gt(0))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let aggregated = page.items().first().or_fail()?;
    expect_that!(aggregated.id, eq(&vehicle_id));
    expect_that!(aggregated, has_aggregate("speed", AggregationMethod::TotalCount, nth(0, period().value(4))));
    Ok(())
}
