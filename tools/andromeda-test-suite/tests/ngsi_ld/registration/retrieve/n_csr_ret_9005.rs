//! Each scope of a registration reads back as written.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    problem::answered,
    raw_answer::send_raw,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{UncheckedDocument, operation::RegisterContextSource},
    model::RawJson,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Context Source Registration of a registration whose scope holds a Scope without a leading slash
/// and one with answers both as written
/// Clause: 4.6.4, 4.18, 5.10.1
#[gtest]
#[tokio::test]
async fn n_csr_ret_9005_each_scope_of_a_registration_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id("spelled")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending(".+", ".*")?)])
            .build()?])
        .build()?;
    // A scope without a leading slash is no Scope of the model, so `scope` is written as text.
    let scope: RawJson = format!(r#"["T{namespace}", "/T{namespace}/Gardens"]"#).parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("scope", &scope);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // The scope spellings under test are no Scope of the model, so the answer is read as received.
    let read = request_to(&broker, Method::GET, &format!("csourceRegistrations/{registration_id}"))?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let response = send_raw(read).await?;
    expect_that!(response.status(), eq(StatusCode::OK));
    expect_that!(response.body(), json_count("$.scope", 2));
    expect_that!(response.body(), json_equals("$.scope[0]", format!("T{namespace}")));
    expect_that!(response.body(), json_equals("$.scope[1]", format!("/T{namespace}/Gardens")));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
