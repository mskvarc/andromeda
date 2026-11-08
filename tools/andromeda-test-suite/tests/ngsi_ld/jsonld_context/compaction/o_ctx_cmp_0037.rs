//! A body rendered under a linked context reads as the stored entity.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::SuiteConfig,
    core_context::core_entry,
    entity_matcher::{has_attribute, member_count},
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, Entity, EntityTypeName, NgsiLdMediaType, OneOrMany},
};

/// Pins: Retrieve Entity as application/ld+json under a Link naming a URL ending with the core, a user @context
/// without it, one setting its own @vocab after it and one defining terms after it answers the short names under a
/// @context that expands them to the stored IRIs, the undefined colour through the core @vocab
/// Clause: 4.4, 5.2.3, 5.5.7
/// Tenant: private (the case reads what the tenant stored under each request @context, and clause 4.14 keeps it the
/// tenant's own)
/// Recorded: clause 5.2.3 has an application/ld+json body name the user @context and the core @context, and clause
/// 4.4 has the core treated last; Andromeda names the request @contexts in order with every core reference removed,
/// and appends the v1.9 core unless what remains leaves it in force last (Annex C.8)
/// Recorded: the Rust case hands the body to an independent JSON-LD 1.1 processor; the file pins everything that
/// fixes what such a processor reads: the exact @context named, the documents behind it (served by the file), the
/// short names in the body, and the IRIs stored
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0037_a_body_rendered_under_a_linked_context_reads_as_the_stored_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let core = config.core_context();
    let served = contexts.url("")?;
    let tenant_linked = tenant.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let stored_type = format!("https://test.andromeda/{namespace}/Device").parse::<EntityTypeName>()?;
    let speed_iri = format!("https://test.andromeda/{namespace}/speed");
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
        .entity_type(device_type.clone())
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
    expect_that!(entity.entity_type, eq(&OneOrMany::one(stored_type)));
    expect_that!(entity, has_attribute(&speed_iri, any_attribute().value(3)));
    expect_that!(entity, has_attribute("colour", any_attribute().value("red")));
    expect_that!(member_count(entity), eq(4));
    for (path, expected) in [
        ("jc/user/wrapper.jsonld", Context::Single(contexts.reference("jc/user/wrapper.jsonld")?)),
        (
            "jc/user/terms.jsonld",
            Context::Multiple(vec![contexts.reference("jc/user/terms.jsonld")?, core_entry()?]),
        ),
        (
            "jc/user/vocab-after-core.jsonld",
            Context::Multiple(vec![contexts.reference("jc/user/vocab-after-core.jsonld")?, core_entry()?]),
        ),
        (
            "jc/user/importing.jsonld",
            Context::Multiple(vec![contexts.reference("jc/user/importing.jsonld")?, core_entry()?]),
        ),
    ] {
        let linked = tenant.with_context(Context::Single(contexts.reference(path)?))?;
        let rendered = linked
            .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
            .entities()
            .retrieve(&device_id)
            .send()
            .await?;
        let entity_2 = rendered.body();
        expect_that!(entity_2.entity_type, eq(&OneOrMany::one(device_type.clone())));
        expect_that!(entity_2, has_attribute("speed", any_attribute().value(3)));
        expect_that!(entity_2, has_attribute("colour", any_attribute().value("red")));
        expect_that!(member_count(entity_2), eq(5));
        expect_that!(entity_2.context.as_ref(), some(eq(&expected)));
    }
    Ok(())
}
