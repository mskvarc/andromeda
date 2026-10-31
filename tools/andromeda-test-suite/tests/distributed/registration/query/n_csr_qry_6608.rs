//! Pick keeps the named members and omit drops them keeping id and type.

use andromeda_test_suite::{
    body_json::{json_absent, json_equals, json_exists},
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    raw_answer::send_raw,
    raw_call::target,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, header::ACCEPT};
use ngsi_ld::registration::{CSourceRegistration, EntityInfo, RegistrationInfo};

/// Pins: Query Context Source Registrations by type with pick endpoint answers 200 with a registration holding id,
/// type and endpoint and no information, and with omit endpoint one holding id, type and information and no
/// endpoint
/// Clause: 5.10.2.3, 4.21
#[gtest]
#[tokio::test]
async fn n_csr_qry_6608_pick_keeps_the_named_members_and_omit_drops_them_keeping_id_and_type() -> Result<()> {
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
    // A projection that drops `information` or `endpoint` leaves a registration the model cannot read, so the answers are read as received.
    let picked = request_to(
        &broker,
        Method::GET,
        &target("csourceRegistrations", &[("type", type_text.as_str()), ("pick", "endpoint")]),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let response = send_raw(picked).await?;
    expect_that!(response.body(), json_exists(&own));
    expect_that!(response.body(), json_equals(&format!("{own}.type"), "ContextSourceRegistration"));
    expect_that!(response.body(), json_exists(&format!("{own}.endpoint")));
    expect_that!(response.body(), json_absent(&format!("{own}.information")));
    let omitted = request_to(
        &broker,
        Method::GET,
        &target("csourceRegistrations", &[("type", type_text.as_str()), ("omit", "endpoint")]),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let response_2 = send_raw(omitted).await?;
    expect_that!(response_2.body(), json_exists(&own));
    expect_that!(response_2.body(), json_equals(&format!("{own}.type"), "ContextSourceRegistration"));
    expect_that!(response_2.body(), json_absent(&format!("{own}.endpoint")));
    expect_that!(response_2.body(), json_exists(&format!("{own}.information")));
    Ok(())
}
