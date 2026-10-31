//! Context source info reads back in written order.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, ContextSourceInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration with `contextSourceInfo` holding the pairs z=1 and a=2, and Retrieve
/// Context Source Registration answers the two pairs in written order
/// Clause: 5.2.9, 5.2.22
#[gtest]
#[tokio::test]
async fn n_csr_cre_0022_context_source_info_reads_back_in_written_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([building_type.as_str()]).build()?])
        .build()?;
    // The model refuses it (a registration contextSourceInfo), so `contextSourceInfo` is written as text.
    let context_source_info: RawJson = r#"[{"key": "z", "value": "1"}, {"key": "a", "value": "2"}]"#.parse()?;
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
        registration_2.context_source_info().and_then(|infos| infos.first()).map(ContextSourceInfo::key),
        some(eq("z"))
    );
    expect_that!(
        registration_2.context_source_info().and_then(|infos| infos.first()),
        some(eq(&ContextSourceInfo::parse("z", "1")?))
    );
    expect_that!(
        registration_2.context_source_info().and_then(|infos| infos.get(1)).map(ContextSourceInfo::key),
        some(eq("a"))
    );
    expect_that!(
        registration_2.context_source_info().and_then(|infos| infos.get(1)),
        some(eq(&ContextSourceInfo::parse("a", "2")?))
    );
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
