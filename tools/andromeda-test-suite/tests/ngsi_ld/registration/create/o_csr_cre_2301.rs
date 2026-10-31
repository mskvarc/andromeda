//! A location with mixed arity positions is refused.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    problem::{answered, problem_among},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Register Context Source whose location, `observationSpace` or `operationSpace` holds a `LineString` mixing
/// two- and three-ordinate positions is refused with a 400 and stores nothing
/// Clause: 4.7.1, 5.9.2, 5.5.4
/// Recorded: RFC 7946 section 3.1.1 constrains each position on its own, so it permits a geometry whose positions
/// differ in arity, and 4.7.1 does not forbid it, so the specification leaves the answer open; the user ruled on
/// 2026-10-06 that such a geometry must be rejected on every path that takes a geometry; no clause names the error
/// type, so `BadRequestData` and `InvalidRequest` are both accepted
#[gtest]
#[tokio::test]
async fn o_csr_cre_2301_a_location_with_mixed_arity_positions_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_location_id = namespace.registration_id("location")?;
    let building_type = namespace.entity_type("Building")?;
    let registration_observation_id = namespace.registration_id("observation")?;
    let registration_operation_id = namespace.registration_id("operation")?;
    let registration_location = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_location_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .build()?;
    // The model refuses it (a registration location), so `location` is written as text.
    let location: RawJson = r#"{"type": "LineString", "coordinates": [[13.4, 52.52], [13.5, 52.53, 250.0]]}"#.parse()?;
    let document = UncheckedDocument::try_from(registration_location.render())?.set_member("location", &location);
    let request = broker
        .unchecked(&RegisterContextSource {
            registration: &registration_location,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem_among(StatusCode::BAD_REQUEST, [ErrorType::BadRequestData, ErrorType::InvalidRequest])
    );
    let registration_observation = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_observation_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .build()?;
    // The model refuses it (a registration observationSpace), so `observationSpace` is written as text.
    let observation_space: RawJson = r#"{"type": "LineString", "coordinates": [[13.4, 52.52], [13.5, 52.53, 250.0]]}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_observation.render())?.set_member("observationSpace", &observation_space);
    let request_2 = broker
        .unchecked(&RegisterContextSource {
            registration: &registration_observation,
        })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem_among(StatusCode::BAD_REQUEST, [ErrorType::BadRequestData, ErrorType::InvalidRequest])
    );
    let registration_operation = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_operation_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .build()?;
    // The model refuses it (a registration operationSpace), so `operationSpace` is written as text.
    let operation_space: RawJson = r#"{"type": "LineString", "coordinates": [[13.4, 52.52], [13.5, 52.53, 250.0]]}"#.parse()?;
    let document_3 = UncheckedDocument::try_from(registration_operation.render())?.set_member("operationSpace", &operation_space);
    let request_3 = broker
        .unchecked(&RegisterContextSource {
            registration: &registration_operation,
        })?
        .document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(
        &answer_3,
        problem_among(StatusCode::BAD_REQUEST, [ErrorType::BadRequestData, ErrorType::InvalidRequest])
    );
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_location_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    Ok(())
}
