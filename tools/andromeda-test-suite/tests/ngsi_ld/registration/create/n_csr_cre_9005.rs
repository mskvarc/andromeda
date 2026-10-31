//! A registration spelled with at id and at type is created.

use andromeda_test_suite::{
    body_json::{json_absent, json_equals},
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
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{NgsiLdMediaType, OneOrMany, RawJson},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration whose id, type and entity selector type are written as @id and @type
/// answers 201, and the registration reads back with id and type
/// Clause: 4.4, 5.9.2
/// Tenant: private (its registration's selector is written with `@type` and matches every query of the tenant)
#[gtest]
#[tokio::test]
async fn n_csr_cre_9005_a_registration_spelled_with_at_id_and_at_type_is_created() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id("keywords")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(vehicle_type.clone())]).build()?])
        .build()?;
    // The keyword spellings `@id` and `@type` are not members the model writes, so the body is written as text.
    let information: RawJson = format!(r#"[{{"entities": [{{"@type": "{vehicle_type}"}}]}}]"#).parse()?;
    let document = UncheckedDocument::try_from(registration.render())?
        .remove_member("id")
        .remove_member("type")
        .set_text("@id", &registration_id.to_string())
        .set_text("@type", "ContextSourceRegistration")
        .set_member("information", &information);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(retrieved.id(), eq(&registration_id));
    expect_that!(
        retrieved
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .map(EntityInfo::entity_type),
        some(eq(&OneOrMany::one(vehicle_type.clone())))
    );
    // Which keyword the answer spells, `type` or `@type`, is a fact of the bytes, so it is read as received.
    let read = request_to(&broker, Method::GET, &format!("csourceRegistrations/{registration_id}"))?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let response = send_raw(read).await?;
    expect_that!(response.status(), eq(StatusCode::OK));
    expect_that!(response.body(), json_equals("$.type", "ContextSourceRegistration"));
    expect_that!(response.body(), json_absent("$['@type']"));
    Ok(())
}
