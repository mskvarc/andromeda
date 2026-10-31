//! A single link to any core release is answered under the current one.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::SuiteConfig,
    core_context::core_entry,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    wire::header_values,
};
use googletest::prelude::*;
use http::header::LINK;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::headers::{Accept, Link},
    model::{Context, ContextEntry, Entity, NgsiLdMediaType},
};

/// Pins: Retrieve Entity under a Link naming the v1.8, v1.6 or v1.3 core, the versionless alias or the v1.9 core
/// answers 200 under the v1.9 core alone, as json in its one @context link and as ld+json in its body
/// Clause: 4.4, 6.3.5, 6.3.6
/// Recorded: clause 4.4 names one core @context; Andromeda reads every embedded older release and the versionless
/// alias as it and names only v1.9
#[gtest]
#[tokio::test]
async fn o_ctx_exp_0035_a_single_link_to_any_core_release_is_answered_under_the_current_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let linked = broker.with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld".to_owned(),
    )?)))?;
    let linked_2 = broker.with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld".to_owned(),
    )?)))?;
    let linked_3 = broker.with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.3.jsonld".to_owned(),
    )?)))?;
    let linked_4 = broker.with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld".to_owned(),
    )?)))?;
    let linked_5 = broker.with_context(Context::Single(core_entry()?))?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let headed = linked
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
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
        some(eq(config.core_context().as_str()))
    );
    let retrieved_2 = linked
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    expect_that!(entity_2, has_attribute("speed", any_attribute().value(1)));
    let headed_2 = linked_2
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
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
        some(eq(config.core_context().as_str()))
    );
    let retrieved_4 = linked_2
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity_4 = retrieved_4.body();
    expect_that!(entity_4.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    expect_that!(entity_4, has_attribute("speed", any_attribute().value(1)));
    let headed_3 = linked_3
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
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
    let retrieved_6 = linked_3
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity_6 = retrieved_6.body();
    expect_that!(entity_6.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    expect_that!(entity_6, has_attribute("speed", any_attribute().value(1)));
    let headed_4 = linked_4
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
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
        some(eq(config.core_context().as_str()))
    );
    let retrieved_8 = linked_4
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity_8 = retrieved_8.body();
    expect_that!(entity_8.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    expect_that!(entity_8, has_attribute("speed", any_attribute().value(1)));
    let headed_5 = linked_5
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(header_values(headed_5.head.headers(), &LINK), len(eq(1)));
    expect_that!(headed_5.head.typed::<Link>().valid().map(|link| link.context_urls().count()), some(eq(1)));
    expect_that!(
        headed_5
            .head
            .typed::<Link>()
            .valid()
            .and_then(|link| link.context_urls().next().map(|url| url.as_str().to_owned())),
        some(eq(config.core_context().as_str()))
    );
    let retrieved_10 = linked_5
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity_10 = retrieved_10.body();
    expect_that!(entity_10.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    expect_that!(entity_10, has_attribute("speed", any_attribute().value(1)));
    Ok(())
}
