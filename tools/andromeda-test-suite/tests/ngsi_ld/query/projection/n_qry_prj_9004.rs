//! A query counts and pages only the entities a pick leaves a member.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, NgsiLdMediaType},
    query::projection::Projection,
};

/// Pins: Query Entities over three Buildings, one without name, with count, limit 1 and pick=id,name serves the
/// three on three full pages counted 3, and with pick=name serves two pages counted 2 and an empty third page
/// Clause: 4.21, 5.5.9, 5.7.2.4, 6.3.13
#[gtest]
#[tokio::test]
async fn n_qry_prj_9004_a_query_counts_and_pages_only_the_entities_a_pick_leaves_a_member() -> Result<()> {
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
        .pick("id,name".parse::<Projection>()?)
        .count(ResultsCount::Include)
        .limit(1)
        .offset(0)
        .send()
        .await?;
    let picked0_id = page.items().first().or_fail()?.id.clone().or_fail()?;
    expect_that!(page.total(), some(eq(3)));
    expect_that!(page.items(), len(eq(1)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .pick("id,name".parse::<Projection>()?)
        .count(ResultsCount::Include)
        .limit(1)
        .offset(1)
        .send()
        .await?;
    let picked1_id = page_2.items().first().or_fail()?.id.clone().or_fail()?;
    expect_that!(page_2.total(), some(eq(3)));
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, not(has_id(&picked0_id))));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .pick("id,name".parse::<Projection>()?)
        .count(ResultsCount::Include)
        .limit(1)
        .offset(2)
        .send()
        .await?;
    page_3.items().first().or_fail()?.id.clone().or_fail()?;
    expect_that!(page_3.total(), some(eq(3)));
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, not(has_id(&picked0_id))));
    expect_that!(page_3.items(), nth(0, not(has_id(&picked1_id))));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .pick("id,name".parse::<Projection>()?)
        .count(ResultsCount::Include)
        .limit(1)
        .offset(3)
        .send()
        .await?;
    expect_that!(page_4.total(), some(eq(3)));
    expect_that!(page_4.items(), len(eq(0)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .pick("name".parse::<Projection>()?)
        .count(ResultsCount::Include)
        .limit(1)
        .offset(0)
        .send()
        .await?;
    expect_that!(page_5.total(), some(eq(2)));
    expect_that!(page_5.items(), len(eq(1)));
    let item = page_5.items().first().or_fail()?;
    expect_that!(item.id, none());
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .pick("name".parse::<Projection>()?)
        .count(ResultsCount::Include)
        .limit(1)
        .offset(1)
        .send()
        .await?;
    expect_that!(page_6.total(), some(eq(2)));
    expect_that!(page_6.items(), len(eq(1)));
    let item_2 = page_6.items().first().or_fail()?;
    expect_that!(item_2.id, none());
    let page_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .pick("name".parse::<Projection>()?)
        .count(ResultsCount::Include)
        .limit(1)
        .offset(2)
        .send()
        .await?;
    expect_that!(page_7.total(), some(eq(2)));
    expect_that!(page_7.items(), len(eq(0)));
    Ok(())
}
