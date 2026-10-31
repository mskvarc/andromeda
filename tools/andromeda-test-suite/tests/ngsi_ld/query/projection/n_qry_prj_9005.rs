//! A query counts and pages only the entities an omit leaves a member.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, NgsiLdMediaType},
    query::projection::Projection,
};

/// Pins: Query Entities over three Buildings, one holding only category, with count, limit 1 and
/// omit=id,type,category serves two pages counted 2 and an empty third page
/// Clause: 4.21, 5.5.9, 5.7.2.4, 6.3.13
#[gtest]
#[tokio::test]
async fn n_qry_prj_9005_a_query_counts_and_pages_only_the_entities_an_omit_leaves_a_member() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_emptied_a_id = namespace.urn("Building", "emptied-a")?;
    let building_type = namespace.entity_type("Building")?;
    let building_emptied_b_id = namespace.urn("Building", "emptied-b")?;
    let building_emptied_c_id = namespace.urn("Building", "emptied-c")?;
    let building_emptied_a = Entity::builder()
        .id(building_emptied_a_id)
        .entity_type(building_type.clone())
        .property("name", "Depot")
        .build()?;
    broker.entities().create(&building_emptied_a).send().await?.done()?;
    let building_emptied_b = Entity::builder()
        .id(building_emptied_b_id)
        .entity_type(building_type.clone())
        .property("category", "Depot")
        .build()?;
    broker.entities().create(&building_emptied_b).send().await?.done()?;
    let building_emptied_c = Entity::builder()
        .id(building_emptied_c_id)
        .entity_type(building_type.clone())
        .property("name", "Depot")
        .build()?;
    broker.entities().create(&building_emptied_c).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .omit("id,type,category".parse::<Projection>()?)
        .count(ResultsCount::Include)
        .limit(1)
        .offset(0)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(2)));
    expect_that!(page.items(), len(eq(1)));
    let item = page.items().first().or_fail()?;
    expect_that!(item.id, none());
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .omit("id,type,category".parse::<Projection>()?)
        .count(ResultsCount::Include)
        .limit(1)
        .offset(1)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(2)));
    expect_that!(page_2.items(), len(eq(1)));
    let item_2 = page_2.items().first().or_fail()?;
    expect_that!(item_2.id, none());
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .omit("id,type,category".parse::<Projection>()?)
        .count(ResultsCount::Include)
        .limit(1)
        .offset(2)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(2)));
    expect_that!(page_3.items(), len(eq(0)));
    Ok(())
}
