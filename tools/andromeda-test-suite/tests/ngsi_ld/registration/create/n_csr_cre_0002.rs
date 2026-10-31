//! Context source info reads back as written.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, ContextSourceInfo, EntityInfo, RegistrationInfo},
};

/// Pins: a registration created with a `contextSourceInfo` array of a reserved key and a custom key reads back with
/// both key-value pairs unchanged
/// Clause: 5.9.2, 5.10.1, 5.2.9, 5.2.22, 4.3.6.5
#[gtest]
#[tokio::test]
async fn n_csr_cre_0002_context_source_info_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model refuses it (a registration contextSourceInfo), so `contextSourceInfo` is written as text.
    let context_source_info: RawJson = r#"[{"key": "accept", "value": "application/ld+json"}, {"key": "X-Custom-Auth", "value": "token-abc"}]"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("contextSourceInfo", &context_source_info);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(registration_2.context_source_info().map(<[_]>::len), some(eq(2)));
    expect_that!(
        registration_2.context_source_info(),
        some(contains(eq(&ContextSourceInfo::parse("accept", "application/ld+json")?)))
    );
    expect_that!(
        registration_2.context_source_info(),
        some(contains(eq(&ContextSourceInfo::parse("X-Custom-Auth", "token-abc")?)))
    );
    Ok(())
}
