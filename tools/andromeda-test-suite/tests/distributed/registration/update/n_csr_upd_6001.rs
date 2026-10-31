//! The id of a registration cannot be changed.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::ErrorType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Update Context Source Registration with a fragment holding another id leaves the registration under its
/// own id and creates none under the proposed one
/// Clause: 5.9.3, 5.2.9
#[gtest]
#[tokio::test]
async fn n_csr_upd_6001_the_id_of_a_registration_cannot_be_changed() -> Result<()> {
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
        .description("original")
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // The fragment writes no `id`, so the update document is written as text.
    let other_id = namespace.registration_id(2)?;
    let request = json_request_to(
        &broker,
        Method::PATCH,
        &format!("csourceRegistrations/{registration_id}"),
        &format!("{{\"id\": \"{other_id}\"}}"),
    )?;
    broker.send_unchecked(request).await?;
    let retrieved = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(retrieved.id(), eq(&registration_id));
    expect_that!(retrieved.description(), some(eq("original")));
    let absent = broker
        .with_responses()
        .with_accept(Accept::any())
        .registrations()
        .retrieve(&other_id)
        .send()
        .await;
    expect_that!(&absent, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
