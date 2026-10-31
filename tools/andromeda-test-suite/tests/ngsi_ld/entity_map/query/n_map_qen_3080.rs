//! Attrs naming a full iri creates the entity map under a user context.

use andromeda_test_suite::{
    broker,
    header_matcher::present,
    hosted_context::HostedContext,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    received_status::ExpectedStatus,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Context, ContextEntry, Entity, InlineContext, NgsiLdMediaType},
};

/// Pins: Create `EntityMap` for Query Entities with attrs as the only selector selects by an attribute named with
/// its full IRI under a Link header carrying a user context, and the `EntityMap` holds the entity
/// Clause: 5.5.7, 5.14.4.4, 5.7.2.4
/// Tenant: private (its queries select by attributes, q or geometry alone, so the broker reads every entity and
/// forwards to every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_map_qen_3080_attrs_naming_a_full_iri_creates_the_entity_map_under_a_user_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let context = Context::Single(ContextEntry::Inline(
        format!(
            r#"{{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "subCategory": "https://test.andromeda/{namespace}/subCategory"}}"#
        )
        .parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("subCategory", "tourism")
            .build()?,
        Entity::builder().id(building_2_id).entity_type(building_type.clone()).build()?,
    ];
    linked.entities().batch_create(&entities).send().await?.done()?;
    // 5.14.4.4 selects as 5.7.2.4 does; 5.5.7: an absolute IRI expands to itself
    let entity_map_request = EntityMapRequest::returned();
    let page = linked
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .attrs(vec![format!("https://test.andromeda/{namespace}/subCategory").parse()?])
        .entity_map(&entity_map_request)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page.entity_map(), present());
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_id)));
    let map = page.entity_map().valid().or_fail()?;
    let map_id = map.id().or_fail()?;
    let map_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(&map_id)
        .send()
        .await?;
    expect_that!(&map_2.id, eq(&map_id));
    expect_that!(&map_2.entity_map, len(eq(1)));
    expect_that!(map_2.entity_map.get(&building_id), some(anything()));
    Ok(())
}
