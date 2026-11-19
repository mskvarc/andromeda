//! A registration endpoint that is no absolute URI says an absolute http or https URI is expected.

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
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{RegisterContextSource, UpdateRegistration},
    },
    model::{ErrorType, NgsiLdMediaType},
    registration::{CSourceRegistration, EntityInfo, RegistrationFragment, RegistrationInfo, RegistrationTypeMember},
};

/// Pins: Create Context Source Registration with `endpoint` `nope` answers 400 `BadRequestData` whose detail names
/// `endpoint` and says an absolute `http` or `https` URI is expected, and stores nothing; Update Context Source
/// Registration to the same `endpoint` answers the same and leaves the stored endpoint as it was
/// Clause: 5.9.2.4, 5.9.3.4, 6.2, Table 5.2.9-1
/// Recorded: clause 5.5.4 fixes the error type but not the wording; Andromeda names the member and the URIs it takes,
/// and not the URI parser's reason
#[gtest]
#[tokio::test]
async fn o_cmn_val_9041_a_registration_endpoint_that_is_no_absolute_uri_says_an_absolute_http_or_https_uri_is_expected() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let information = || RegistrationInfo::builder().entities([EntityInfo::new(vehicle_type.clone())]).build();
    let detail = "the request body member 'endpoint' does not decode: endpoint must be an absolute http or https URI, the schemes of an NGSI-LD API root";
    let refused_id = namespace.registration_id("refused")?;
    let refused = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(refused_id.clone())
        .information([information()?])
        .build()?;
    // The model refuses it (no URI), so `endpoint` is written as text.
    let document = UncheckedDocument::try_from(refused.render())?.set_text("endpoint", "nope");
    let request = broker.unchecked(&RegisterContextSource { registration: &refused })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail(detail));
    let missing = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&refused_id)
        .send()
        .await;
    expect_that!(&missing, answered(StatusCode::NOT_FOUND));
    let stored_id = namespace.registration_id("stored")?;
    let stored = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(stored_id.clone())
        .information([information()?])
        .build()?;
    broker.registrations().register(&stored).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&stored_id);
    let fragment = RegistrationFragment::builder().type_member(RegistrationTypeMember::Omitted).build()?;
    // The model refuses it (no URI), so `endpoint` is written as text.
    let document = UncheckedDocument::try_from(fragment.render())?.set_text("endpoint", "nope");
    let request = broker
        .unchecked(&UpdateRegistration {
            id: &stored_id,
            fragment: &fragment,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail(detail));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&stored_id)
        .send()
        .await?;
    expect_that!(retrieved.endpoint(), eq(config.unreachable_origin()));
    Ok(())
}
