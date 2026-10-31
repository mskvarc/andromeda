//! A shorthand name in a fragment is expanded through the request context.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    core_context::core_entry,
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, InlineContext},
    registration::{CSourceRegistration, EntityInfo, RegistrationFragment, RegistrationInfo, RegistrationTypeMember},
};

/// Pins: Update Context Source Registration with a fragment naming a property by a term of the Link context answers
/// 204, and the registration reads back with that property as its IRI without a Link and as the term under that
/// Link
/// Clause: 5.9.3.4, 5.5.7
#[gtest]
#[tokio::test]
async fn n_csr_upd_6005_a_shorthand_name_in_a_fragment_is_expanded_through_the_request_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let context = Context::Multiple(vec![
        ContextEntry::Inline(format!(r#"{{"speed_{namespace}": "https://test.andromeda/{namespace}/speed"}}"#).parse::<InlineContext>()?),
        core_entry()?,
    ]);
    let ctx = HostedContext::add(&broker, &context).await?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let fragment = RegistrationFragment::builder()
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names([namespace.attribute("speed")?.to_string()])
            .build()?])
        .type_member(RegistrationTypeMember::Omitted)
        .build()?;
    let answer = linked.with_responses().registrations().update(&registration_id, &fragment).send().await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let registration_2 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.property_names().first())
            .map(ToString::to_string)
            .as_deref(),
        some(eq(format!("https://test.andromeda/{namespace}/speed").as_str()))
    );
    let registration_3 = linked.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(
        registration_3
            .information()
            .first()
            .and_then(|info| info.property_names().first())
            .map(ToString::to_string)
            .as_deref(),
        some(eq(format!("speed_{namespace}").as_str()))
    );
    Ok(())
}
