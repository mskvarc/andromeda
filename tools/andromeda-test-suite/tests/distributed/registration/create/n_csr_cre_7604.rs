//! An auxiliary registration advertising query temporal is bad request data.

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
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration on broker A of an auxiliary registration naming broker B and
/// advertising `queryTemporal` answers 400 `BadRequestData` and stores nothing
/// Clause: 5.9.2.4, 4.3.6.3, 4.20
#[gtest]
#[tokio::test]
async fn n_csr_cre_7604_an_auxiliary_registration_advertising_query_temporal_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    // The model refuses it (the document breaks the rules of its data type), so `mode` is written as text.
    let mode: RawJson = r#""auxiliary""#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("mode", &mode);
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
