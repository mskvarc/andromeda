//! An endpoint that is no http or https URI is refused.

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
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};
use url::Url;

/// Pins: Create Context Source Registration with an `endpoint` of `ftp://x/y` or `file:///etc/passwd` answers 400
/// `BadRequestData` whose detail names `endpoint`, `http` and `https`, and stores nothing, while one with an `http` or
/// an `https` endpoint answers 201 and reads the endpoint back
/// Clause: 5.9.2.4, 6.2, Table 5.2.9-1
#[gtest]
#[tokio::test]
async fn n_csr_cre_9008_an_endpoint_that_is_no_http_or_https_uri_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let information = || RegistrationInfo::builder().entities([EntityInfo::new(vehicle_type.clone())]).build();
    for (name, endpoint) in [("ftp", "ftp://x/y"), ("file", "file:///etc/passwd")] {
        let registration_id = namespace.registration_id(name)?;
        let registration = CSourceRegistration::builder(endpoint)
            .id(registration_id.clone())
            .information([information()?])
            .build()?;
        let answer = broker.with_responses().registrations().register(&registration).send().await;
        expect_that!(
            &answer,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
                .with_detail_containing("endpoint")
                .with_detail_containing("http")
                .with_detail_containing("https"),
            "{endpoint}"
        );
        let missing = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .registrations()
            .retrieve(&registration_id)
            .send()
            .await;
        expect_that!(&missing, answered(StatusCode::NOT_FOUND), "{endpoint}");
    }
    let https_endpoint: Url = format!("https://{}/", config.unreachable_origin().authority()).parse()?;
    for (name, endpoint) in [("http", config.unreachable_origin()), ("https", &https_endpoint)] {
        let registration_id = namespace.registration_id(name)?;
        let registration = CSourceRegistration::builder(endpoint.as_str())
            .id(registration_id.clone())
            .information([information()?])
            .build()?;
        let answer = broker.with_responses().registrations().register(&registration).send().await;
        expect_that!(&answer, answered(StatusCode::CREATED), "{endpoint}");
        // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
        // cooldown that opens there holds back a later test's registration naming the same endpoint.
        let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .registrations()
            .retrieve(&registration_id)
            .send()
            .await?;
        expect_that!(retrieved.endpoint(), eq(endpoint));
    }
    Ok(())
}
