//! An import cycle is refused and the broker keeps serving.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::Accept,
        operation::{ContextKindFilter, QueryMethod},
    },
    model::{Context, Entity, EntityFragment, ErrorType, NgsiLdMediaType},
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Retrieve Entity, Create Entity and Update Attributes under a Link naming a definition importing a second
/// one that imports the first, and Create Entity and Query Entities by POST as application/ld+json naming it in the
/// body, are each refused as `BadRequestData`; the broker keeps answering, nothing is written and no wrapper is
/// hosted
/// Clause: 5.5.6, 6.3.5, JSON-LD 1.1 API s4.1.2
/// Tenant: private (the case asserts what the tenant holds afterwards, and clause 4.14 keeps it the tenant's own)
/// Recorded: clause 5.5.6 makes a remote @context that is invalid `BadRequestData`; Andromeda reads a @context
/// whose references never end (the JSON-LD 1.1 API context overflow) as invalid
#[gtest]
#[tokio::test]
async fn o_ctx_exp_0039_an_import_cycle_is_refused_and_the_broker_keeps_serving() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let served = contexts.url("")?;
    let tenant_linked = tenant.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let tenant_linked_2 = tenant.with_context(Context::Single(contexts.reference("jc/import-cycle/a.jsonld")?))?;
    let device_refused_id = namespace.urn("Device", "refused")?;
    let tenant_body_context = tenant
        .with_context(Context::Multiple(vec![contexts.reference("jc/import-cycle/a.jsonld")?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let device_body_id = namespace.urn("Device", "body")?;
    let tenant_body_context_2 = tenant
        .with_context(Context::Single(contexts.reference("jc/import-cycle/a.jsonld")?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
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
    contexts
        .serve(
            "jc/import-cycle/b.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "@import": "{served}jc/import-cycle/a.jsonld",
        "rate": "https://test.andromeda/{namespace}/rate"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/import-cycle/a.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "Device_{namespace}": "https://test.andromeda/{namespace}/Device",
        "speed": "https://test.andromeda/{namespace}/speed",
        "@import": "{served}jc/import-cycle/b.jsonld"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 3)
        .build()?;
    tenant_linked.entities().create(&device).send().await?.done()?;
    let answer = tenant_linked_2
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .retrieve(&device_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = tenant_linked
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&device_id));
    let device_refused = Entity::builder().id(device_refused_id.clone()).entity_type(device_type.clone()).build()?;
    let answer_2 = tenant_linked_2.with_responses().entities().create(&device_refused).send().await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved_2 = tenant_linked
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.id, eq(&device_id));
    let fragment = EntityFragment::builder().property("speed", 2).build()?;
    let answer_3 = tenant_linked_2
        .with_responses()
        .entities()
        .update_attributes(&device_id, &fragment)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved_3 = tenant_linked
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3.id, eq(&device_id));
    let device_body = Entity::builder().id(device_body_id.clone()).entity_type(device_type.clone()).build()?;
    let answer_4 = tenant_body_context.with_responses().entities().create(&device_body).send().await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_5 = tenant_body_context_2
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(format!("https://test.andromeda/{namespace}/Device").as_str())
        .send()
        .await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved_4 = tenant_linked
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity_4 = retrieved_4.body();
    expect_that!(entity_4.id, eq(&device_id));
    let retrieved_5 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity_5 = retrieved_5.body();
    expect_that!(
        entity_5,
        has_attribute(&format!("https://test.andromeda/{namespace}/speed"), any_attribute().value(3))
    );
    let answer_6 = tenant
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .retrieve(&device_refused_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    let answer_7 = tenant
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .retrieve(&device_body_id)
        .send()
        .await;
    expect_that!(&answer_7, answered(StatusCode::NOT_FOUND));
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
