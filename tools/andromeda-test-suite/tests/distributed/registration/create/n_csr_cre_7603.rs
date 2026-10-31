//! An operation outside the named operations is bad request data and stores nothing.

use andromeda_test_suite::{
    broker,
    config::PeerRole,
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

/// Pins: Create Context Source Registration on broker A naming broker B with operations holding `notAnOperation`
/// answers 400 `BadRequestData`, and the registration does not exist afterwards
/// Clause: 5.9.2, 5.2.9, 4.20
#[gtest]
#[tokio::test]
async fn n_csr_cre_7603_an_operation_outside_the_named_operations_is_bad_request_data_and_stores_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model refuses it (`notAnOperation` is not a valid value of `operations` at operations), so `operations` is written as text.
    let operations: RawJson = r#"["notAnOperation"]"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("operations", &operations);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::any())
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
