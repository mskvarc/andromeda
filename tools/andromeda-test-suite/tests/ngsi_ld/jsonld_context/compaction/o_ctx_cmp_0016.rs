//! A post query with one body context answered as json links it.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    core_context::core_context,
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
    wire::header_values,
};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::{Accept, Link},
        operation::{ContextKindFilter, QueryMethod},
    },
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: Query Entities by POST answered as json under a body @context that is one URL ending with the core
/// @context, as a string or a one-element array, links that URL, and under the core @context links the core; the
/// tenant holds no wrapper
/// Clause: 6.3.6, 5.13.1, Annex C.8
/// Tenant: private (the case asserts which `ImplicitlyCreated` @contexts the tenant holds, and clause 4.14 keeps
/// them the tenant's own)
/// Recorded: Andromeda links a request @context that is one URL leaving the core @context in force last (Annex C.8)
/// as that URL, and hosts nothing for it
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0016_a_post_query_with_one_body_context_answered_as_json_links_it() -> Result<()> {
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
    let tenant_body_context = tenant
        .with_context(Context::Single(contexts.reference("jc/user/wrapper.jsonld")?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let tenant_body_context_2 = tenant
        .with_context(Context::Multiple(vec![contexts.reference("jc/user/wrapper.jsonld")?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let tenant_core_context_body = tenant.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
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
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).property("speed", 3).build()?;
    tenant_linked.entities().create(&device).send().await?.done()?;
    let headed = tenant_body_context
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(format!("https://test.andromeda/{namespace}/Device").as_str())
        .ids(vec![device_id.clone()])
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
    let headed_2 = tenant_body_context_2
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(format!("https://test.andromeda/{namespace}/Device").as_str())
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
    let headed_3 = tenant_core_context_body
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(format!("https://test.andromeda/{namespace}/Device").as_str())
        .ids(vec![device_id.clone()])
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
        some(eq(config.core_context().as_str()))
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
