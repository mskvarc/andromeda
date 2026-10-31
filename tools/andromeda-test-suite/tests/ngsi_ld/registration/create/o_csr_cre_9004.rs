//! A top level jsonld context is a context source property.

use andromeda_test_suite::{
    body_json::json_equals,
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
    registration::{CSourceRegistration, EntityInfo, RegistrationId, RegistrationInfo},
};

/// Pins: A registration body carrying a top-level `jsonldContext` member is created whatever the member holds, an
/// unretrievable URL or an object, nothing is dereferenced, and the member reads back as written
/// Clause: 4.3.6.6, Table 5.2.9-1, 5.9.2.4
/// Recorded: Table 5.2.9-1 defines no `jsonldContext` member, the forwarding context being the `jsonldContext` key
/// of `contextSourceInfo` (clause 4.3.6.6); Andromeda keeps any other member as a Context Source Property, so a
/// top-level `jsonldContext` is data the registration carries and not a context
#[gtest]
#[tokio::test]
async fn o_csr_cre_9004_a_top_level_jsonld_context_is_a_context_source_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let url_id = namespace.registration_id("url")?;
    let object_id = namespace.registration_id("object")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let absent_url = format!(
        "{}/contexts/{namespace}/absent.jsonld",
        config.unreachable_origin().origin().ascii_serialization()
    );
    let members: [(&RegistrationId, RawJson); 2] = [(&url_id, format!("\"{absent_url}\"").parse()?), (&object_id, r#"{"kept": true}"#.parse()?)];
    for (registration_id, jsonld_context) in members {
        let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
            .id(registration_id.clone())
            .information([RegistrationInfo::builder().entities([EntityInfo::new(vehicle_type.clone())]).build()?])
            .build()?;
        // `jsonldContext` is no member of the registration, which the model keeps only as a property, so it is written as text.
        let document = UncheckedDocument::try_from(registration.render())?.set_member("jsonldContext", &jsonld_context);
        let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, answered(StatusCode::CREATED));
        // The additional member is read as received, as the model keeps it only as a property.
        let read = request_to(&broker, Method::GET, &format!("csourceRegistrations/{registration_id}"))?
            .append_header(ACCEPT, HeaderValue::from_static("application/json"));
        let response = send_raw(read).await?;
        expect_that!(response.status(), eq(StatusCode::OK));
        if registration_id == &url_id {
            expect_that!(response.body(), json_equals("$.jsonldContext", absent_url.clone()));
        } else {
            expect_that!(response.body(), json_equals("$.jsonldContext.kept", true));
        }
    }
    broker.registrations().delete(&url_id).send().await?;
    broker.registrations().delete(&object_id).send().await?;
    Ok(())
}
