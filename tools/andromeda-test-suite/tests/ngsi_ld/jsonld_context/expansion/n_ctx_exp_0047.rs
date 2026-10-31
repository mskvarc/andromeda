//! A request naming one context is answered with one context link.

use andromeda_test_suite::{broker, config::SuiteConfig, core_context::core_entry, namespace::Namespace, outcome::Done, wire::header_values};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    http::headers::{Accept, Link},
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: Retrieve Entity as application/json under one Link naming the core answers 200 with exactly one @context
/// link, to the core
/// Clause: 6.3.6, JSON-LD 1.1 s6.1
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0047_a_request_naming_one_context_is_answered_with_one_context_link() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let linked = broker.with_context(Context::Single(core_entry()?))?;
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
    Ok(())
}
