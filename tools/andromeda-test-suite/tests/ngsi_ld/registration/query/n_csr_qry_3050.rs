//! A boolean after an ordering operator in csf is bad request data.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    problem::{answered, problem},
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    query::q,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations refuses a csf whose ordering operator is followed by true or false with
/// `BadRequestData`, since csf is a query string as per clause 4.9, and answers the Boolean after ==
/// Clause: 4.9, 5.10.2.4, 6.8.3.2
#[gtest]
#[tokio::test]
async fn n_csr_qry_3050_a_boolean_after_an_ordering_operator_in_csf_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(sensor_type.clone()).with_id_pattern(namespace.id_pattern("Sensor")?)])
            .build()?])
        .build()?;
    // The model refuses it (a registration a registration property), so `enabled` is written as text.
    let enabled: RawJson = r#"{"type": "Property", "value": true}"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("enabled", &enabled);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    // Table 6.8.3.2-1: csf is "Context Source filter as per clause 4.9"
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("csourceRegistrations?type=Sensor_{namespace}&csf=enabled%3Etrue"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("csourceRegistrations?type=Sensor_{namespace}&csf=enabled%3C%3Dfalse"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // the same csf with == is answered
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&sensor_type)
        .csf(q::attr("enabled").eq(true))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&registration_id)));
    Ok(())
}
