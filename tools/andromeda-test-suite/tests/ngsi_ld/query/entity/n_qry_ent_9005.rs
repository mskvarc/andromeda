//! Post query entity selectors are a union not a cross product.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::QueryMethod,
    model::Entity,
    query::{EntitySelector, builders::SelectorTypeBuilder},
};

/// Pins: Query Entities by POST with the selectors id A type Vehicle and id B type Building answers only Entity B,
/// not an Entity of id A and type Building
/// Clause: 5.2.23, 5.2.8, 5.7.2.4
/// Tenant: private (the broker forwards a query of several selectors to registrations of other types too, so in the
/// shared tenant it reaches other files' sources)
#[gtest]
#[tokio::test]
async fn n_qry_ent_9005_post_query_entity_selectors_are_a_union_not_a_cross_product() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let vehicle_union_id = namespace.urn("Vehicle", "union")?;
    let building_type = namespace.entity_type("Building")?;
    let building_union_id = namespace.urn("Building", "union")?;
    let vehicle_union = Entity::builder().id(vehicle_union_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&vehicle_union).send().await?.done()?;
    let building_union = Entity::builder().id(building_union_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building_union).send().await?.done()?;
    let selectors = [
        EntitySelector::new(SelectorTypeBuilder::from(&namespace.entity_type("Vehicle")?).build()?).with_ids(vehicle_union_id.clone()),
        EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_union_id.clone()),
    ];
    let page = broker.entities().query().method(QueryMethod::Post).selectors(selectors).send().await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_union_id)));
    Ok(())
}
