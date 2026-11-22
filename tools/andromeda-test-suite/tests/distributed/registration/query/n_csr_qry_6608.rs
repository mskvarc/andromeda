//! Pick and omit leave the whole registration in the answer.

use andromeda_test_suite::{
    body_json::{json_equals, json_exists},
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    raw_call::{call, target},
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::{CSourceRegistration, EntityInfo, RegistrationInfo};
use std::iter::once;

/// Pins: Query Context Source Registrations by type with pick endpoint, and with omit endpoint, answers 200 with the
/// whole registration (id, type, endpoint and information), as the same query without them does
/// Clause: 5.10.2.3, 5.10.2.4, 6.8.3.2
#[gtest]
#[tokio::test]
async fn n_csr_qry_6608_pick_and_omit_leave_the_whole_registration_in_the_answer() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let own = format!("$[?(@.id=='{registration_id}')]");
    let type_text = vehicle_type.to_string();
    // The typed client has no pick or omit on registrations, so the query string is written as given.
    for projection in [None, Some(("pick", "endpoint")), Some(("omit", "endpoint"))] {
        let parameters: Vec<(&str, &str)> = once(("type", type_text.as_str())).chain(projection).collect();
        let response = call(
            &broker,
            Method::GET,
            &target("csourceRegistrations", &parameters),
            &[("Accept", "application/json")],
            None,
        )
        .await?;
        expect_that!(response.status(), eq(StatusCode::OK), "{projection:?}");
        expect_that!(response.body(), json_exists(&own), "{projection:?}");
        expect_that!(response.body(), json_equals(&format!("{own}.type"), "ContextSourceRegistration"));
        expect_that!(response.body(), json_exists(&format!("{own}.endpoint")));
        expect_that!(response.body(), json_exists(&format!("{own}.information")));
    }
    Ok(())
}
