//! A context ending with the core context is linked as itself.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
    wire::header_values,
};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    http::{
        headers::{Accept, Link},
        operation::ContextKindFilter,
    },
    model::{Context, Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: A json Retrieve Entity and Query Entities under one URL ending with the core @context, directly, through a
/// chain, or through a relative reference, link that URL itself, and the tenant holds no wrapper
/// Clause: 6.3.6, 5.13.1, Annex C.8
/// Tenant: private (the case asserts which `ImplicitlyCreated` @contexts the tenant holds, and clause 4.14 keeps
/// them the tenant's own)
/// Recorded: Andromeda links a request @context that is one URL leaving the core @context in force last (Annex C.8)
/// as that URL, and hosts nothing for it
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0015_a_context_ending_with_the_core_context_is_linked_as_itself() -> Result<()> {
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
    let tenant_linked_2 = tenant.with_context(Context::Single(contexts.reference("jc/user/wrapper.jsonld")?))?;
    let tenant_linked_3 = tenant.with_context(Context::Single(contexts.reference("jc/user/outer.jsonld")?))?;
    let tenant_linked_4 = tenant.with_context(Context::Single(contexts.reference("jc/user/nested/relative.jsonld")?))?;
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
            "jc/user/outer.jsonld",
            &format!(
                r#"{{
      "@context": "{served}jc/user/wrapper.jsonld"
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/user/nested/wrapped.jsonld",
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
            "jc/user/nested/relative.jsonld",
            &format!(
                r#"{{
      "@context": [
        {{
          "rate": "https://test.andromeda/{namespace}/rate"
        }},
        "wrapped.jsonld"
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
        .build()?;
    tenant_linked.entities().create(&device).send().await?.done()?;
    let headed = tenant_linked_2
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(header_values(headed.head.headers(), &LINK), len(eq(1)));
    expect_that!(headed.head.typed::<Link>().valid().map(|link| link.context_urls().count()), some(eq(1)));
    expect_that!(
        headed
            .head
            .typed::<Link>()
            .valid()
            .and_then(|link| link.context_urls().next().map(|url| url.as_str().to_owned())),
        some(eq(contexts.url("jc/user/wrapper.jsonld")?.as_str()))
    );
    let headed_2 = tenant_linked_2
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&device_type)
        .ids(vec![device_id.clone()])
        .send()
        .await?;
    expect_that!(header_values(headed_2.head.headers(), &LINK), len(eq(1)));
    expect_that!(headed_2.head.typed::<Link>().valid().map(|link| link.context_urls().count()), some(eq(1)));
    expect_that!(
        headed_2
            .head
            .typed::<Link>()
            .valid()
            .and_then(|link| link.context_urls().next().map(|url| url.as_str().to_owned())),
        some(eq(contexts.url("jc/user/wrapper.jsonld")?.as_str()))
    );
    let headed_3 = tenant_linked_3
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(header_values(headed_3.head.headers(), &LINK), len(eq(1)));
    expect_that!(headed_3.head.typed::<Link>().valid().map(|link| link.context_urls().count()), some(eq(1)));
    expect_that!(
        headed_3
            .head
            .typed::<Link>()
            .valid()
            .and_then(|link| link.context_urls().next().map(|url| url.as_str().to_owned())),
        some(eq(contexts.url("jc/user/outer.jsonld")?.as_str()))
    );
    let headed_4 = tenant_linked_4
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    expect_that!(header_values(headed_4.head.headers(), &LINK), len(eq(1)));
    expect_that!(headed_4.head.typed::<Link>().valid().map(|link| link.context_urls().count()), some(eq(1)));
    expect_that!(
        headed_4
            .head
            .typed::<Link>()
            .valid()
            .and_then(|link| link.context_urls().next().map(|url| url.as_str().to_owned())),
        some(eq(contexts.url("jc/user/nested/relative.jsonld")?.as_str()))
    );
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
