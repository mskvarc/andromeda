//! A rendered body written back under its own context stores the same iris.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::SuiteConfig,
    entity_matcher::{has_attribute, member_count},
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, Entity, EntityTypeName, NgsiLdMediaType, OneOrMany},
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: An application/ld+json Retrieve Entity answer, under each of four request @contexts, written back as a new
/// Entity under nothing but the @context it names stores the type and attribute IRIs of the original
/// Clause: 4.4, 5.2.3, 6.3.5
/// Tenant: private (the case reads what the tenant stored under each request @context, and clause 4.14 keeps it the
/// tenant's own)
#[gtest]
#[tokio::test]
async fn n_ctx_cmp_0039_a_rendered_body_written_back_under_its_own_context_stores_the_same_iris() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let core = config.core_context();
    let served = contexts.url("")?;
    let tenant_linked = tenant.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let stored_type = format!("https://test.andromeda/{namespace}/Device").parse::<EntityTypeName>()?;
    let speed_iri = format!("https://test.andromeda/{namespace}/speed");
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
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
            "jc/user/wrapper.jsonld",
            &format!(
                r#"{{
      "@context": [
        "{served}jc/user/terms.jsonld",
        "{core}"
      ]
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/user/vocab-after-core.jsonld",
            &format!(
                r#"{{
      "@context": [
        "{served}jc/user/terms.jsonld",
        "{core}",
        {{
          "@vocab": "https://test.andromeda/{namespace}/vocab/"
        }}
      ]
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/user/importing.jsonld",
            &format!(
                r#"{{
      "@context": [
        "{core}",
        {{
          "Device_{namespace}": "https://test.andromeda/{namespace}/Device",
          "speed": "https://test.andromeda/{namespace}/speed"
        }}
      ]
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type)
        .property("speed", 3)
        .property("colour", "red")
        .build()?;
    tenant_linked.entities().create(&device).send().await?.done()?;
    let retrieved = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.entity_type, eq(&OneOrMany::one(stored_type.clone())));
    expect_that!(entity, has_attribute(&speed_iri, any_attribute().value(3)));
    expect_that!(entity, has_attribute("colour", any_attribute().value("red")));
    expect_that!(member_count(entity), eq(4));
    for (copy, context) in [
        (2, "jc/user/wrapper.jsonld"),
        (3, "jc/user/terms.jsonld"),
        (4, "jc/user/vocab-after-core.jsonld"),
        (5, "jc/user/importing.jsonld"),
    ] {
        let linked = tenant.with_context(Context::Single(contexts.reference(context)?))?;
        let rendered = linked
            .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
            .entities()
            .retrieve(&device_id)
            .send()
            .await?;
        let original = rendered.body();
        expect_that!(original.context.as_ref(), some(anything()));
        let named = original.context.clone().or_fail()?;
        // The retrieved entity is borrowed from the answer, and the copy takes the identifier of the new entity.
        let mut written_back = original.clone().into_input()?;
        written_back.id = namespace.urn("Device", copy)?;
        let body_context = tenant.with_context(named)?.with_context_delivery(ContextDelivery::LdJsonBody);
        body_context.entities().create(&written_back).send().await?.done()?;
        let stored = tenant
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&namespace.urn("Device", copy)?)
            .send()
            .await?;
        let stored_entity = stored.body();
        expect_that!(stored_entity.id, eq(&namespace.urn("Device", copy)?));
        expect_that!(stored_entity.entity_type, eq(&OneOrMany::one(stored_type.clone())));
        expect_that!(stored_entity, has_attribute(&speed_iri, any_attribute().value(3)));
        expect_that!(stored_entity, has_attribute("colour", any_attribute().value("red")));
        expect_that!(member_count(stored_entity), eq(4));
    }
    Ok(())
}
