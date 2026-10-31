//! An exclusive registration must name an entity id and attributes.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{ErrorType, RawJson},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration with mode exclusive by `idPattern`, by type only, and by an Entity id
/// without attribute names answers 400 `BadRequestData` each, and none of them exists
/// Clause: 4.3.6.3, 5.9.2.4
/// Recorded: 4.3.6.3 requires an exclusive registration to define an entity id and attributes but names no error
/// type for one that does not; Andromeda answers `BadRequestData` as for any registration breaking the clause 5.2.9
/// shape
#[gtest]
#[tokio::test]
async fn o_csr_cre_6401_an_exclusive_registration_must_name_an_entity_id_and_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration_3_id = namespace.registration_id(3)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .build()?;
    // The model refuses it (the document breaks the rules of its data type), so `mode` is written as text.
    let mode: RawJson = r#""exclusive""#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("mode", &mode);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_2 = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([vehicle_type.as_str()])
            .property_names(["speed"])
            .build()?])
        .build()?;
    // The model refuses it (the document breaks the rules of its data type), so `mode` is written as text.
    let mode_2: RawJson = r#""exclusive""#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_2.render())?.set_member("mode", &mode_2);
    let request_2 = broker
        .unchecked(&RegisterContextSource { registration: &registration_2 })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_3 = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_3_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id)])
            .build()?])
        .build()?;
    // The model refuses it (the document breaks the rules of its data type), so `mode` is written as text.
    let mode_3: RawJson = r#""exclusive""#.parse()?;
    let document_3 = UncheckedDocument::try_from(registration_3.render())?.set_member("mode", &mode_3);
    let request_3 = broker
        .unchecked(&RegisterContextSource { registration: &registration_3 })?
        .document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::any())
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let answer_5 = broker
        .with_responses()
        .with_accept(Accept::any())
        .registrations()
        .retrieve(&registration_2_id)
        .send()
        .await;
    expect_that!(&answer_5, answered(StatusCode::NOT_FOUND));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::any())
        .registrations()
        .retrieve(&registration_3_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    Ok(())
}
