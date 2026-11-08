//! A user context without the core is linked through one hosted wrapper.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_details,
    core_context::core_entry,
    entity_matcher::has_attribute,
    minted_wrapper::{minted_wrapper, served_document},
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ContextKindFilter},
    model::{Context, Entity, EntityTypeName, NgsiLdMediaType, OneOrMany, StoredContextKind},
};

/// Pins: A json Retrieve Entity under a user @context without the core links one wrapper with an absolute minted URL
/// naming no tenant and carries no body @context; the wrapper dereferences under the tenant header to the user
/// @context followed by the core, is listed `ImplicitlyCreated`, and the entity holds the IRIs the user @context gives
/// Clause: 6.3.6, 5.13.1, 5.13.3.4, 4.14
/// Tenant: private (the case asserts which `ImplicitlyCreated` @contexts the tenant holds, and clause 4.14 keeps them
/// the tenant's own)
/// Recorded: clause 6.3.6 has an application/json answer link its @context but does not say how a @context that no
/// single URL names is linked; Andromeda hosts the value an application/ld+json body would carry as an
/// `ImplicitlyCreated` @context (clause 5.13.1) and links its absolute URL
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0008_a_user_context_without_the_core_is_linked_through_one_hosted_wrapper() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let tenant_linked = tenant.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let stored_type = format!("https://test.andromeda/{namespace}/Device").parse::<EntityTypeName>()?;
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
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 3)
        .build()?;
    tenant_linked.entities().create(&device).send().await?.done()?;
    let headed = tenant_linked
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let wrapper = minted_wrapper(&headed.head)?;
    let retrieved = &headed.outcome;
    let entity = retrieved.body();
    expect_that!(entity.context.as_ref(), none());
    expect_that!(entity.entity_type, eq(&OneOrMany::one(device_type)));
    let document = served_document(&tenant, &wrapper).await?;
    expect_that!(document.extra_members, is_empty());
    expect_that!(
        document.context,
        eq(&Context::Multiple(vec![contexts.reference("jc/user/terms.jsonld")?, core_entry()?]))
    );
    let listing = context_details::list_details(&tenant, Some(ContextKindFilter::ImplicitlyCreated)).await?;
    let listed = context_details::metadata_list(&listing)?;
    expect_that!(
        listed.iter().find(|entry| entry.local_id == wrapper.id).map(|entry| &entry.kind),
        some(eq(&StoredContextKind::ImplicitlyCreated))
    );
    let retrieved_2 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.entity_type, eq(&OneOrMany::one(stored_type)));
    expect_that!(
        entity_2,
        has_attribute(&format!("https://test.andromeda/{namespace}/speed"), any_attribute().value(3))
    );
    Ok(())
}
