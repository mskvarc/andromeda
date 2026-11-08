//! A tenant's wrapper is served and listed only for that tenant.

use andromeda_test_suite::{
    broker,
    core_context::core_entry,
    minted_wrapper::{listed_wrapper_ids, minted_wrapper, served_document},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::ContextKindFilter},
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: A wrapper linked for a non-default tenant names no tenant in its URL, dereferences under that tenant's
/// header, answers 404 without a header and under another tenant's, and is listed by its own tenant only
/// Clause: 4.14, 6.3.14, 6.3.6, 5.13.1
/// Tenant: private (the subject is how a wrapper of one tenant is kept from the others, clauses 4.14 and 6.3.14)
/// Recorded: clause 6.3.6 has an application/json answer link its @context but does not say how a @context that no
/// single URL names is linked; Andromeda hosts the value an application/ld+json body would carry as an
/// `ImplicitlyCreated` @context (clause 5.13.1) and links its absolute URL
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0017_a_tenants_wrapper_is_served_and_listed_only_for_that_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let other_tenant = broker::provisioned_tenant(&broker, format!("{namespace}-other").parse()?, &namespace).await?;
    let tenant_linked = tenant.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
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
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).property("speed", 3).build()?;
    tenant_linked.entities().create(&device).send().await?.done()?;
    let headed = tenant_linked
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let wrapper = minted_wrapper(&headed.head)?;
    let document = served_document(&tenant, &wrapper).await?;
    expect_that!(document.extra_members, is_empty());
    expect_that!(
        document.context,
        eq(&Context::Multiple(vec![contexts.reference("jc/user/terms.jsonld")?, core_entry()?]))
    );
    let answer = broker.with_responses().with_accept(Accept::any()).contexts().serve(&wrapper.id).send().await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    let answer_2 = other_tenant
        .with_responses()
        .with_accept(Accept::any())
        .contexts()
        .serve(&wrapper.id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let listed = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::ImplicitlyCreated)
        .send()
        .await?;
    expect_that!(listed_wrapper_ids(&listed)?, elements_are![eq(&wrapper.id)]);
    let listed_2 = other_tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::ImplicitlyCreated)
        .send()
        .await?;
    expect_that!(listed_wrapper_ids(&listed_2)?, is_empty());
    let answer_3 = broker.with_responses().with_accept(Accept::any()).contexts().serve(&wrapper.id).send().await;
    expect_that!(&answer_3, answered(StatusCode::NOT_FOUND));
    Ok(())
}
