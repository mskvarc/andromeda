//! A matching source answering 404 leaves the evolution not found in every mode.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    body_json::json_absent,
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    problem::problem,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::ErrorType,
    query::temporal_query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Retrieve Temporal Evolution of an Entity no broker holds, whose inclusive, exclusive and redirect sources
/// each answer 404, is answered 404 `ResourceNotFound` with no representation synthesised from the registration
/// Clause: 5.7.3.4, 6.3.17
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7400_a_matching_source_answering_404_leaves_the_evolution_not_found_in_every_mode() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_3_id = namespace.registration_id(3)?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let registration = CSourceRegistration::builder(source.endpoint("inclusive")?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .operations([DistributedOperation::RetrieveTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    source
        .answer(
            Method::GET,
            &format!("/inclusive/ngsi-ld/v1/temporal/entities/{vehicle_id}"),
            source_answer::status(StatusCode::NOT_FOUND).insert_header(CONTENT_TYPE, HeaderValue::from_static("application/json")),
        )
        .await?;
    let registration_2 = CSourceRegistration::builder(source.endpoint("exclusive")?)
        .id(registration_2_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_2_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([DistributedOperation::RetrieveTemporal])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    source
        .answer(
            Method::GET,
            &format!("/exclusive/ngsi-ld/v1/temporal/entities/{vehicle_2_id}"),
            source_answer::status(StatusCode::NOT_FOUND).insert_header(CONTENT_TYPE, HeaderValue::from_static("application/json")),
        )
        .await?;
    let registration_3 = CSourceRegistration::builder(source.endpoint("redirect")?)
        .id(registration_3_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_3_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::RetrieveTemporal])
        .build()?;
    broker.registrations().register(&registration_3).send().await?;
    source
        .answer(
            Method::GET,
            &format!("/redirect/ngsi-ld/v1/temporal/entities/{vehicle_3_id}"),
            source_answer::status(StatusCode::NOT_FOUND).insert_header(CONTENT_TYPE, HeaderValue::from_static("application/json")),
        )
        .await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    // The problem document is read as received to pin that no entity member was synthesised beside it.
    let body = answer.answered_body().map(ToString::to_string).or_fail()?;
    expect_that!(body.as_bytes(), json_absent("$.id"));
    let forwarded = source
        .received_matching(&SourceRequest::matching(Method::GET, "/inclusive/ngsi-ld/v1/temporal/entities/.*"))
        .await?;
    expect_that!(forwarded, len(eq(1)));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_2_id)
        .temporal_q(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let body_2 = answer_2.answered_body().map(ToString::to_string).or_fail()?;
    expect_that!(body_2.as_bytes(), json_absent("$.id"));
    let forwarded_2 = source
        .received_matching(&SourceRequest::matching(Method::GET, "/exclusive/ngsi-ld/v1/temporal/entities/.*"))
        .await?;
    expect_that!(forwarded_2, len(eq(1)));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_3_id)
        .temporal_q(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let body_3 = answer_3.answered_body().map(ToString::to_string).or_fail()?;
    expect_that!(body_3.as_bytes(), json_absent("$.id"));
    let forwarded_3 = source
        .received_matching(&SourceRequest::matching(Method::GET, "/redirect/ngsi-ld/v1/temporal/entities/.*"))
        .await?;
    expect_that!(forwarded_3, len(eq(1)));
    Ok(())
}
