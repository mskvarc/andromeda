//! A paginated json query names one context link.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, outcome::Done, remote_context::RemoteContexts, wire::header_values};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    http::{
        headers::{Accept, Link},
        operation::ResultsCount,
    },
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: A json Query Entities with limit 1 and count over three entities, under a user @context with and without
/// the core @context, answers 200 with a next link and exactly one @context link
/// Clause: 6.3.6, 5.5.9, JSON-LD 1.1 s6.1
#[gtest]
#[tokio::test]
async fn n_ctx_cmp_0002_a_paginated_json_query_names_one_context_link() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let core = config.core_context();
    let served = contexts.url("")?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device_2_id = namespace.urn("Device", 2)?;
    let device_3_id = namespace.urn("Device", 3)?;
    let linked_2 = broker.with_context(Context::Single(contexts.reference("jc/user/wrapper.jsonld")?))?;
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
    let device = Entity::builder().id(device_id).entity_type(device_type.clone()).property("speed", 3).build()?;
    linked.entities().create(&device).send().await?.done()?;
    let device_2 = Entity::builder()
        .id(device_2_id)
        .entity_type(device_type.clone())
        .property("speed", 3)
        .build()?;
    linked.entities().create(&device_2).send().await?.done()?;
    let device_3 = Entity::builder()
        .id(device_3_id)
        .entity_type(device_type.clone())
        .property("speed", 3)
        .build()?;
    linked.entities().create(&device_3).send().await?.done()?;
    let headed = linked
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&device_type)
        .limit(1)
        .count(ResultsCount::Include)
        .send()
        .await?;
    let page = headed.outcome;
    expect_that!(header_values(headed.head.headers(), &LINK), len(eq(1)));
    expect_that!(headed.head.typed::<Link>().valid().map(|link| link.context_urls().count()), some(eq(1)));
    expect_that!(page.next(), some(anything()));
    let headed_2 = linked_2
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&device_type)
        .limit(1)
        .count(ResultsCount::Include)
        .send()
        .await?;
    let page_2 = headed_2.outcome;
    expect_that!(header_values(headed_2.head.headers(), &LINK), len(eq(1)));
    expect_that!(headed_2.head.typed::<Link>().valid().map(|link| link.context_urls().count()), some(eq(1)));
    expect_that!(page_2.next(), some(anything()));
    Ok(())
}
