//! An ld json answer never hosts a wrapper.

use andromeda_test_suite::{broker, config::SuiteConfig, header_matcher::absent, namespace::Namespace, outcome::Done, remote_context::RemoteContexts};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::{Accept, Link},
        operation::ContextKindFilter,
    },
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: An application/ld+json Retrieve Entity under a user @context without the core, one importing the core and
/// one naming a chain ending with the core answers 200 with no @context link, and the tenant holds no wrapper
/// Clause: 6.3.6, 5.13.1
/// Tenant: private (the case asserts which `ImplicitlyCreated` @contexts the tenant holds, and clause 4.14 keeps
/// them the tenant's own)
/// Recorded: Andromeda hosts a wrapper only for an answer that names its @context in a Link and has a body
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0005_an_ld_json_answer_never_hosts_a_wrapper() -> Result<()> {
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
    let tenant_linked_2 = tenant.with_context(Context::Single(contexts.reference("jc/user/importing.jsonld")?))?;
    let tenant_linked_3 = tenant.with_context(Context::Single(contexts.reference("jc/user/outer.jsonld")?))?;
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
    contexts
        .serve(
            "jc/user/outer.jsonld",
            &format!(
                r#"{{
      "@context": "{served}jc/user/wrapper.jsonld"
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).property("speed", 3).build()?;
    tenant_linked.entities().create(&device).send().await?.done()?;
    let headed = tenant_linked
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(headed.head.typed::<Link>(), absent());
    let retrieved = &headed.outcome;
    let entity = retrieved.body();
    expect_that!(entity.context.as_ref(), some(anything()));
    let headed_2 = tenant_linked_2
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(headed_2.head.typed::<Link>(), absent());
    let retrieved_2 = &headed_2.outcome;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.context.as_ref(), some(anything()));
    let headed_3 = tenant_linked_3
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(headed_3.head.typed::<Link>(), absent());
    let retrieved_3 = &headed_3.outcome;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3.context.as_ref(), some(anything()));
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
