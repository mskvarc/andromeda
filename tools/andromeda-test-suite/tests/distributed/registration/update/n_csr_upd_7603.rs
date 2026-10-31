//! A mode outside its values is bad request data and an unknown id on delete not found.

use andromeda_test_suite::{broker, config::PeerRole, namespace::Namespace, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::ErrorType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Update Context Source Registration on broker A with mode not-a-mode answers 400 `BadRequestData` and the
/// registration reads back mode inclusive; deleting an unknown registration answers 404 `ResourceNotFound`
/// Clause: 5.9.3.4, 5.9.4.4, 5.2.9
#[gtest]
#[tokio::test]
async fn n_csr_upd_7603_a_mode_outside_its_values_is_bad_request_data_and_an_unknown_id_on_delete_not_found() -> Result<()> {
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
    broker.registrations().register(&registration).send().await?;
    // The fragment holds only modes the model knows, so the update document is written as text.
    let request = json_request_to(
        &broker,
        Method::PATCH,
        &format!("csourceRegistrations/{registration_id}"),
        r#"{"mode": "not-a-mode"}"#,
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(retrieved.mode(), some(eq(RegistrationMode::Inclusive)));
    let unknown_id = namespace.registration_id(2)?;
    let deleted = broker.with_responses().registrations().delete(&unknown_id).send().await;
    expect_that!(&deleted, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
