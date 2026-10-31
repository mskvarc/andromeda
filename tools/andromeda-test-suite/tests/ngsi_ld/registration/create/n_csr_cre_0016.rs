//! An auxiliary registration advertising query temporal is bad request data.

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
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration with mode auxiliary and operations `queryTemporal` answers 400
/// `BadRequestData` and the registration does not exist
/// Clause: 5.9.2.4, 4.20
#[gtest]
#[tokio::test]
async fn n_csr_cre_0016_an_auxiliary_registration_advertising_query_temporal_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id(vehicle_id)])
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
