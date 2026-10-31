//! Type and attribute names are expanded with the request context.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, Entity, InlineContext, NgsiLdMediaType},
    registration::EntitySource,
};

/// Pins: Create `EntityMap` for Query Entities with a type and an attrs selector spelled with terms of a user
/// context maps the Entity stored under the expanded IRIs, and the same spelling without that context maps nothing
/// Clause: 5.5.7, 5.14.4.4
#[gtest]
#[tokio::test]
async fn n_map_qen_7005_type_and_attribute_names_are_expanded_with_the_request_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Vehicle_{namespace}": "https://test.andromeda/{namespace}/Vehicle", "speed_{namespace}": "https://test.andromeda/{namespace}/speed"}}"#)
            .parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property(namespace.attribute("speed")?, 20)
        .build()?;
    linked.entities().create(&vehicle).send().await?.done()?;
    let created = linked
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .attrs(vec![format!("speed_{namespace}").parse()?])
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(1)));
    expect_that!(map.entity_map.get(&vehicle_id), some(contains(eq(&EntitySource::Local))));
    let created_2 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .attrs(vec![format!("speed_{namespace}").parse()?])
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map_2 = created_2.body();
    expect_that!(&map_2.entity_map, len(eq(0)));
    Ok(())
}
