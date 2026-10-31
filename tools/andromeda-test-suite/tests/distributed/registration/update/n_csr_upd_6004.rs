//! A fragment breaking the registration definition is bad request data.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::ErrorType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Update Context Source Registration with a fragment holding mode foo answers 400 `BadRequestData`, and the
/// mode reads back unchanged
/// Clause: 5.9.3.4, 5.2.9
#[gtest]
#[tokio::test]
async fn n_csr_upd_6004_a_fragment_breaking_the_registration_definition_is_bad_request_data() -> Result<()> {
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
    // The fragment holds only modes the model knows, so the update document is written as text.
    let request = json_request_to(&broker, Method::PATCH, &format!("csourceRegistrations/{registration_id}"), r#"{"mode": "foo"}"#)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(retrieved.mode(), some(eq(RegistrationMode::Inclusive)));
    Ok(())
}
