//! An update to an endpoint that is no http or https URI is refused.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    registration::{CSourceRegistration, EntityInfo, RegistrationFragment, RegistrationInfo, RegistrationTypeMember},
};
use url::Url;

/// Pins: Update Context Source Registration of an `http` endpoint to `ftp://x/y` or `file:///etc/passwd` answers 400
/// `BadRequestData` whose detail names `endpoint`, `http` and `https`, and the registration keeps its endpoint, while
/// an update to an `https` endpoint answers 204 and reads that endpoint back
/// Clause: 5.9.3.4, 6.2, Table 5.2.9-1
#[gtest]
#[tokio::test]
async fn n_csr_upd_9004_an_update_to_an_endpoint_that_is_no_http_or_https_uri_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(vehicle_type)]).build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    for endpoint in ["ftp://x/y", "file:///etc/passwd"] {
        let fragment = RegistrationFragment::builder()
            .endpoint(endpoint)
            .type_member(RegistrationTypeMember::Omitted)
            .build()?;
        let answer = broker.with_responses().registrations().update(&registration_id, &fragment).send().await;
        expect_that!(
            &answer,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
                .with_detail_containing("endpoint")
                .with_detail_containing("http")
                .with_detail_containing("https"),
            "{endpoint}"
        );
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .registrations()
            .retrieve(&registration_id)
            .send()
            .await?;
        expect_that!(retrieved.endpoint(), eq(config.unreachable_origin()), "{endpoint}");
    }
    let https_endpoint: Url = format!("https://{}/", config.unreachable_origin().authority()).parse()?;
    let fragment = RegistrationFragment::builder()
        .endpoint(https_endpoint.as_str())
        .type_member(RegistrationTypeMember::Omitted)
        .build()?;
    let answer = broker.with_responses().registrations().update(&registration_id, &fragment).send().await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(retrieved.endpoint(), eq(&https_endpoint));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
