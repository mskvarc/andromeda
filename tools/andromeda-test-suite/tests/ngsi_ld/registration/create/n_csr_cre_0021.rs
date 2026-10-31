//! Management reads back every member as written.

use andromeda_test_suite::{
    body_json::{json_absent, json_equals},
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    problem::answered,
    raw_answer::send_raw,
    registration_update_request::update_without_type,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Duration, NgsiLdMediaType},
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, RegistrationFragment, RegistrationInfo, RegistrationManagementInfo},
};

/// Pins: Create Context Source Registration with management holding `cacheDuration`, cooldown, `localOnly` and
/// timeout, and Retrieve Context Source Registration answers the four members as written; Update Context Source
/// Registration with management holding only timeout replaces the whole member, so the registration then reads back
/// with timeout alone
/// Clause: 5.2.9, 5.2.34, 5.9.3, 5.5.8
#[gtest]
#[tokio::test]
async fn n_csr_cre_0021_management_reads_back_every_member_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(building_type.clone())]).build()?])
        .management(
            RegistrationManagementInfo::builder()
                .locality(Locality::Local)
                .cache_duration("PT10S".parse::<Duration>()?)
                .timeout(3000)
                .cooldown(500)
                .build()?,
        )
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    let management = retrieved.management().or_fail()?;
    expect_that!(management.cache_duration().map(ToString::to_string), some(eq("PT10S")));
    expect_that!(management.cooldown().map(ToString::to_string), some(eq("500")));
    expect_that!(management.locality(), eq(Locality::Local));
    expect_that!(management.timeout().map(ToString::to_string), some(eq("3000")));
    let fragment = RegistrationFragment::builder()
        .management(RegistrationManagementInfo::builder().timeout(1000).build()?)
        .build()?;
    let request = update_without_type(&broker, &registration_id, &fragment)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    let management_2 = retrieved_2.management().or_fail()?;
    expect_that!(management_2.timeout().map(ToString::to_string), some(eq("1000")));
    expect_that!(management_2.cache_duration(), none());
    expect_that!(management_2.cooldown(), none());
    expect_that!(management_2.locality(), eq(Locality::default()));
    // `localOnly` is false by default in the model, so its absence is read as received.
    let read = request_to(&broker, Method::GET, &format!("csourceRegistrations/{registration_id}"))?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let response = send_raw(read).await?;
    expect_that!(response.body(), json_equals("$.management.timeout", 1000));
    expect_that!(response.body(), json_absent("$.management.localOnly"));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
