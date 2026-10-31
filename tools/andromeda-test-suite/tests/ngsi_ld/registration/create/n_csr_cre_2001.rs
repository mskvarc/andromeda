//! A context nested in location or context source info is bad request data.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    context_source::ContextSource,
    core_context::core_context,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration refuses a user @context nested in location, in a `contextSourceInfo`
/// pair and as a string in an entity selector, and stores nothing
/// Clause: 5.5.7, 5.9.2
/// Recorded: 5.5.7 says such parts "should" result in `BadRequestData`; Andromeda refuses them
#[gtest]
#[tokio::test]
async fn n_csr_cre_2001_a_context_nested_in_location_or_context_source_info_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let core_in_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let registration_1_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration_3_id = namespace.registration_id(3)?;
    let info = || -> Result<RegistrationInfo> {
        Ok(RegistrationInfo::builder()
            .entities([EntityInfo::new(device_type.clone()).with_id(device_id.clone())])
            .build()?)
    };
    let registration_1 = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_1_id.clone())
        .information([info()?])
        .build()?;
    // The model refuses a nested `@context`, so each member that carries one is written as text.
    let location: RawJson = r#"{"type": "Point", "coordinates": [13.4, 52.5], "@context": {}}"#.parse()?;
    let document_1 = UncheckedDocument::try_from(registration_1.render())?.set_member("location", &location);
    let request_1 = core_in_body
        .unchecked(&RegisterContextSource { registration: &registration_1 })?
        .document(&document_1);
    let answer_1 = core_in_body.send_unchecked(request_1).await;
    expect_that!(&answer_1, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_2 = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_2_id.clone())
        .information([info()?])
        .build()?;
    let source_info: RawJson = r#"[{"key": "X-Token", "value": "abc", "@context": {}}]"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_2.render())?.set_member("contextSourceInfo", &source_info);
    let request_2 = core_in_body
        .unchecked(&RegisterContextSource { registration: &registration_2 })?
        .document(&document_2);
    let answer_2 = core_in_body.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_3 = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_3_id.clone())
        .information([info()?])
        .build()?;
    let core = SuiteConfig::get()?.core_context();
    let information: RawJson = format!(r#"[{{"entities": [{{"type": "{device_type}", "id": "{device_id}", "@context": "{core}"}}]}}]"#).parse()?;
    let document_3 = UncheckedDocument::try_from(registration_3.render())?.set_member("information", &information);
    let request_3 = core_in_body
        .unchecked(&RegisterContextSource { registration: &registration_3 })?
        .document(&document_3);
    let answer_3 = core_in_body.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    for registration_id in [&registration_1_id, &registration_2_id, &registration_3_id] {
        let answer_4 = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .registrations()
            .retrieve(registration_id)
            .send()
            .await;
        expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    }
    Ok(())
}
