//! A json null is bad request data and the ngsi ld null removes the member.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, ContextSourcePropertyName, EntityInfo, RegistrationInfo},
};

/// Pins: Update Context Source Registration with description null answers 400 `BadRequestData` naming description
/// and keeps it; with description, `expiresAt` and the Context Source Property tier set to urn:ngsi-ld:null answers
/// 204 and reads back without them; with endpoint set to urn:ngsi-ld:null answers 400 `BadRequestData`; Create
/// Context Source Registration with description set to urn:ngsi-ld:null answers 400 `BadRequestData` and stores
/// nothing
/// Clause: 4.5.0, 5.2.1, 5.4, 5.5.8, 5.9.2.4, 5.9.3.4, Table 5.2.9-1
#[gtest]
#[tokio::test]
async fn n_csr_upd_9003_a_json_null_is_bad_request_data_and_the_ngsi_ld_null_removes_the_member() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_1_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let building_type = namespace.entity_type("Building")?;
    let origin = config.unreachable_origin().origin().ascii_serialization();
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_1_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(building_type.clone())]).build()?])
        .description("kept until deleted")
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .build()?;
    // A plain `tier` is a Context Source Property written without its Property wrapper, so it is written as text.
    let tier: RawJson = r#""gold""#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("tier", &tier);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let path = format!("csourceRegistrations/{registration_1_id}");
    // The JSON null and the NGSI-LD Null are no values of a typed fragment, so each body is sent as written.
    let request_2 = json_request_to(&broker, Method::PATCH, &path, r#"{"description": null}"#)?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("description")
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_1_id)
        .send()
        .await?;
    expect_that!(retrieved.description(), some(eq("kept until deleted")));
    let request_3 = json_request_to(&broker, Method::PATCH, &path, r#"{"endpoint": "urn:ngsi-ld:null"}"#)?;
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let request_4 = json_request_to(
        &broker,
        Method::PATCH,
        &path,
        r#"{"description": "urn:ngsi-ld:null", "expiresAt": "urn:ngsi-ld:null", "tier": "urn:ngsi-ld:null"}"#,
    )?;
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, answered(StatusCode::NO_CONTENT));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_1_id)
        .send()
        .await?;
    expect_that!(retrieved_2.description(), none());
    expect_that!(retrieved_2.expires_at(), none());
    expect_that!(retrieved_2.properties().get(&"tier".parse::<ContextSourcePropertyName>()?), none());
    expect_that!(retrieved_2.endpoint().origin().ascii_serialization(), eq(&origin));
    let registration_2 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(building_type.clone())]).build()?])
        .build()?;
    let document_2 = UncheckedDocument::try_from(registration_2.render())?.null_member("description");
    let request_5 = broker
        .unchecked(&RegisterContextSource { registration: &registration_2 })?
        .document(&document_2);
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let missing = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_2_id)
        .send()
        .await;
    expect_that!(&missing, answered(StatusCode::NOT_FOUND));
    broker.registrations().delete(&registration_1_id).send().await?;
    Ok(())
}
