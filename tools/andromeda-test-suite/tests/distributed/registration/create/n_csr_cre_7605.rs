//! A body without information is bad request data and a taken id already exists.

use andromeda_test_suite::{broker, config::PeerRole, namespace::Namespace, problem::problem, refused_body::expect_bad_request_data};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::ErrorType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration on broker A without information and endpoint answers 400
/// `BadRequestData`; under the id of a registration it already holds it answers 409 `AlreadyExists`
/// Clause: 5.9.2.4, 5.2.9
#[gtest]
#[tokio::test]
async fn n_csr_cre_7605_a_body_without_information_is_bad_request_data_and_a_taken_id_already_exists() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    // A registration without `information` and `endpoint` is none the model writes, so the body is written as text.
    let without_members = format!(r#"{{"id":"urn:ngsi-ld:ContextSourceRegistration:{namespace}-2","type":"ContextSourceRegistration"}}"#);
    expect_bad_request_data(&broker, &Method::POST, "csourceRegistrations", &[without_members]).await?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let answer = broker.with_responses().registrations().register(&registration).send().await;
    expect_that!(&answer, problem(StatusCode::CONFLICT, ErrorType::AlreadyExists));
    Ok(())
}
