//! A context source info accept outside the two media types is bad request data.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::ErrorType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Update Context Source Registration with a `contextSourceInfo` pair accept text/plain answers 400
/// `BadRequestData`, and the registration reads back without `contextSourceInfo`
/// Clause: 5.9.3.4, 4.3.6.6
#[gtest]
#[tokio::test]
async fn n_csr_upd_6002_a_context_source_info_accept_outside_the_two_media_types_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // The fragment always writes `type` and a media type the model accepts, so the update document is written as text.
    let request = json_request_to(
        &broker,
        Method::PATCH,
        &format!("csourceRegistrations/{registration_id}"),
        r#"{"contextSourceInfo": [{"key": "accept", "value": "text/plain"}]}"#,
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(retrieved.id(), eq(&registration_id));
    expect_that!(retrieved.context_source_info(), none());
    Ok(())
}
