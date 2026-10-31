//! A malformed geometry is bad request data.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::RegisterContextSource},
    model::{ErrorType, RawJson},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration with a location, `observationSpace` or `operationSpace` that is no
/// `GeoJSON` geometry answers 400 `BadRequestData`
/// Clause: 5.9.2.4, 5.2.9, 4.7
#[gtest]
#[tokio::test]
async fn n_csr_cre_6007_a_malformed_geometry_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration_3_id = namespace.registration_id(3)?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model refuses it (invalid geometry at location), so `location` is written as text.
    let location: RawJson = r#"{"type": "Point", "coordinates": "nope"}"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("location", &location);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_2 = CSourceRegistration::builder(source.endpoint("2")?)
        .id(registration_2_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model refuses it (invalid geometry at observationSpace), so `observationSpace` is written as text.
    let observation_space: RawJson = r#"{"type": "Polygon", "coordinates": 5}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_2.render())?.set_member("observationSpace", &observation_space);
    let request_2 = broker
        .unchecked(&RegisterContextSource { registration: &registration_2 })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_3 = CSourceRegistration::builder(source.endpoint("3")?)
        .id(registration_3_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model refuses it (invalid geometry at operationSpace), so `operationSpace` is written as text.
    let operation_space: RawJson = r#"{"type": "Point"}"#.parse()?;
    let document_3 = UncheckedDocument::try_from(registration_3.render())?.set_member("operationSpace", &operation_space);
    let request_3 = broker
        .unchecked(&RegisterContextSource { registration: &registration_3 })?
        .document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
