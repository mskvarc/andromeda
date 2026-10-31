//! A context source info entry that is not an http header is bad request data and keeps the stored one.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    registration::{CSourceRegistration, ContextSourceInfo, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Update Context Source Registration with a `contextSourceInfo` key with a space or a value with a line break
/// answers 400 `BadRequestData` and keeps the stored `contextSourceInfo`
/// Clause: 6.3.19, 5.9.3.4
#[gtest]
#[tokio::test]
async fn n_csr_upd_7606_a_context_source_info_entry_that_is_not_an_http_header_is_bad_request_data_and_keeps_the_stored_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .context_source_info(("X-Api-Key", "secret"))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Each pair breaks a rule of an HTTP header the model enforces, so the update documents are written as text.
    for pair in [r#"{"key": "Bad Name", "value": "x"}"#, r#"{"key": "X-Api-Key", "value": "a\nb"}"#] {
        let request = json_request_to(
            &broker,
            Method::PATCH,
            &format!("csourceRegistrations/{registration_id}"),
            &format!(r#"{{"contextSourceInfo": [{pair}]}}"#),
        )?;
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
        let kept = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .registrations()
            .retrieve(&registration_id)
            .send()
            .await?;
        expect_that!(
            kept.context_source_info().and_then(|infos| infos.first()),
            some(eq(&ContextSourceInfo::parse("X-Api-Key", "secret")?))
        );
    }
    Ok(())
}
