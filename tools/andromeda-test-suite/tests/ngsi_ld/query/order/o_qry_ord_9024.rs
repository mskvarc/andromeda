//! An entity query no registration matches is ordered without local.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::ordering::OrderBy,
};

/// Pins: Query Entities with `orderBy=name` and no local parameter, over a type no registration covers, answers 200
/// with the Entities ascending by name, and with orderBy=name;desc descending
/// Clause: 4.23.1, 4.23.3, 5.5.13, 5.7.2.4
/// Recorded: 5.7.2.4 and 5.7.4.4 refuse an ordering whose execution is not limited to the local scope (5.5.13),
/// which 5.5.13 defines as using no Context Source Registration; whether a query no registration matches is in that
/// state without local=true is left open. Andromeda admits the ordering when no registration takes part in the
/// query
#[gtest]
#[tokio::test]
async fn o_qry_ord_9024_an_entity_query_no_registration_matches_is_ordered_without_local() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_order_1_id = namespace.urn("Building", "order-1")?;
    let order_scope_type = namespace.entity_type("OrderScope")?;
    let building_order_2_id = namespace.urn("Building", "order-2")?;
    let building_order_3_id = namespace.urn("Building", "order-3")?;
    let building_order_1 = Entity::builder()
        .id(building_order_1_id.clone())
        .entity_type(order_scope_type.clone())
        .property("name", "A")
        .build()?;
    broker.entities().create(&building_order_1).send().await?.done()?;
    let building_order_2 = Entity::builder()
        .id(building_order_2_id.clone())
        .entity_type(order_scope_type.clone())
        .property("name", "B")
        .build()?;
    broker.entities().create(&building_order_2).send().await?.done()?;
    let building_order_3 = Entity::builder()
        .id(building_order_3_id.clone())
        .entity_type(order_scope_type.clone())
        .property("name", "C")
        .build()?;
    broker.entities().create(&building_order_3).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&order_scope_type)
        .ordering("name".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_id(&building_order_1_id)));
    expect_that!(page.items(), nth(1, has_id(&building_order_2_id)));
    expect_that!(page.items(), nth(2, has_id(&building_order_3_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&order_scope_type)
        .ordering("name;desc".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(3)));
    expect_that!(page_2.items(), nth(0, has_id(&building_order_3_id)));
    expect_that!(page_2.items(), nth(1, has_id(&building_order_2_id)));
    expect_that!(page_2.items(), nth(2, has_id(&building_order_1_id)));
    Ok(())
}
