//! Retrieve registration refuses pick and omit.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, problem::problem, raw_call::target, unchecked_call::exchange};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::ErrorType,
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Retrieve Context Source Registration with pick id, and with omit endpoint, answers 400 `InvalidRequest`,
/// while the same request without them answers 200 with the registration
/// Clause: 6.3.20, 6.9.3.1, 5.10.1
#[gtest]
#[tokio::test]
async fn n_csr_ret_9008_retrieve_registration_refuses_pick_and_omit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([vehicle_type.as_str()])
            .property_names(["speed"])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let path = format!("csourceRegistrations/{registration_id}");
    // The typed client has no `pick` or `omit` on registrations, so the query string is written as given.
    for parameters in [vec![("pick", "id")], vec![("omit", "endpoint")]] {
        let request = exchange(&broker, Method::GET, &target(&path, &parameters), &[("Accept", "application/json")], None)?;
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    }
    let retrieved = broker.registrations().retrieve(&registration_id).send().await?;
    expect_that!(retrieved.id(), eq(&registration_id));
    Ok(())
}
