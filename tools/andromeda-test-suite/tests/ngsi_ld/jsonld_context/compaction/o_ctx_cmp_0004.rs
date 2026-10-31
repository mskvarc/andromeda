//! Partial successes and errors name no context and host nothing.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    partial_outcome::reported,
    problem::answered,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::{
        BatchOperationResult,
        UncheckedDocument,
        headers::Accept,
        operation::{BatchUpsert, ContextKindFilter},
    },
    model::{Context, Entity, Lossless, NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: A json Batch Upsert answered 207 and a json Retrieve Entity answered 404, both under a user @context
/// without the core, carry no @context link, and the tenant holds no wrapper afterwards
/// Clause: 6.3.6, 5.13.1, 5.5.4
/// Tenant: private (the case asserts which `ImplicitlyCreated` @contexts the tenant holds, and clause 4.14 keeps
/// them the tenant's own)
/// Recorded: Andromeda hosts a wrapper only for an answer that names its @context in a Link and has a body
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0004_partial_successes_and_errors_name_no_context_and_host_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let tenant_linked = tenant.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device_2_id = namespace.urn("Device", 2)?;
    let device_missing_id = namespace.urn("Device", "missing")?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    contexts
        .serve(
            "jc/user/terms.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "Device_{namespace}": "https://test.andromeda/{namespace}/Device",
        "speed": "https://test.andromeda/{namespace}/speed"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let device = Entity::builder().id(device_id).entity_type(device_type.clone()).property("speed", 1).build()?;
    let document = UncheckedDocument::from_entity(&device, Lossless::Normalized);
    let device_2 = Entity::builder().id(device_2_id).entity_type(device_type.clone()).build()?;
    // The model refuses it (invalid value at speed), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": null}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&device_2, Lossless::Normalized).set_member("speed", &speed);
    let entities = [device, device_2];
    let request = tenant_linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .unchecked(&BatchUpsert::builder().entities(&entities).build())?
        .documents(&[document, document_2]);
    let answer = tenant_linked.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::MULTI_STATUS).without_header(HeaderName::from_static("link")));
    reported::<BatchOperationResult, _>(answer)?;
    let answer_2 = tenant_linked
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_missing_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND).without_header(HeaderName::from_static("link")));
    let contexts = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::ImplicitlyCreated)
        .send()
        .await?;
    expect_that!(contexts, each(not(displays_as(contains_substring("JsonLdContext")))));
    Ok(())
}
