//! A query leaves out the entity a pick empties.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::projection::Projection,
};

/// Pins: Query Entities with pick=name over a Building with a name and one holding only category answers the first
/// alone, and with pick=unknown answers an empty array
/// Clause: 4.21, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_prj_9006_a_query_leaves_out_the_entity_a_pick_empties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_query_kept_id = namespace.urn("Building", "query-kept")?;
    let building_type = namespace.entity_type("Building")?;
    let city_query_kept_id = namespace.urn("City", "query-kept")?;
    let building_query_emptied_id = namespace.urn("Building", "query-emptied")?;
    let building_query_kept = Entity::builder()
        .id(building_query_kept_id)
        .entity_type(building_type.clone())
        .property("name", "Depot")
        .relationship("linkedTo", city_query_kept_id)
        .geo_property("location", Shape::Point(Position::try_new(1.5, 2.5)?))
        .build()?;
    broker.entities().create(&building_query_kept).send().await?.done()?;
    let building_query_emptied = Entity::builder()
        .id(building_query_emptied_id)
        .entity_type(building_type.clone())
        .property("category", "commercial")
        .build()?;
    broker.entities().create(&building_query_emptied).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .pick("name".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_attribute("name", any_attribute())));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .pick("unknown".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    Ok(())
}
