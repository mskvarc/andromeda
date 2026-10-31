//! A management cooldown and timeout above 32 bits read back as written.

use andromeda_test_suite::{body_json::json_equals, broker, config::SuiteConfig, namespace::Namespace, raw_answer::send_raw, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Number},
    registration::{CSourceRegistration, RegistrationInfo, RegistrationManagementInfo},
};

/// Pins: Create Context Source Registration with management cooldown and timeout of 4294967296000 milliseconds, past
/// the 32-bit range, answers 201 and Retrieve Context Source Registration answers both unchanged
/// Clause: 5.2.34, 5.9.2, 5.10.1
#[gtest]
#[tokio::test]
async fn n_csr_cre_6018_a_management_cooldown_and_timeout_above_32_bits_read_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id("wide")?;
    let building_type = namespace.entity_type("Building")?;
    let wide = "4294967296000".parse::<Number>()?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([building_type.as_str()]).build()?])
        .management(RegistrationManagementInfo::builder().cooldown(wide.clone()).timeout(wide).build()?)
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    let management = retrieved.management().or_fail()?;
    expect_that!(management.cooldown().map(ToString::to_string), some(eq("4294967296000")));
    expect_that!(management.timeout().map(ToString::to_string), some(eq("4294967296000")));
    // The typed read keeps the numbers as text, so the answer is read as received to pin them as JSON numbers.
    let read = request_to(&broker, Method::GET, &format!("csourceRegistrations/{registration_id}"))?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let response = send_raw(read).await?;
    expect_that!(response.body(), json_equals("$.management.cooldown", 4_294_967_296_000_u64));
    expect_that!(response.body(), json_equals("$.management.timeout", 4_294_967_296_000_u64));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
