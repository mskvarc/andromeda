//! The post binding points at no page it cannot serve.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{QueryMethod, ResultsCount},
    },
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Query Entities through POST `/entityOperations/query` with limit 2 and count answers the first two of
/// three Entities with NGSILD-Results-Count 3 and no next or prev Link, the same body with offset 2 answers the
/// third, and the GET binding of the same query answers a next Link
/// Clause: 6.3.10, 6.3.13, 5.5.9, 6.23.3.1
/// Recorded: clauses 6.3.10 and 5.5.9 want a pointer a client can dereference without the request body, which no
/// URI can carry for the POST binding; Andromeda emits none there and pages by the URL window
#[gtest]
#[tokio::test]
async fn o_qry_pag_9014_the_post_binding_points_at_no_page_it_cannot_serve() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_page_1_id = namespace.urn("Building", "page-1")?;
    let building_type = namespace.entity_type("Building")?;
    let building_page_2_id = namespace.urn("Building", "page-2")?;
    let building_page_3_id = namespace.urn("Building", "page-3")?;
    let building_page_1 = Entity::builder().id(building_page_1_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building_page_1).send().await?.done()?;
    let building_page_2 = Entity::builder().id(building_page_2_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building_page_2).send().await?.done()?;
    let building_page_3 = Entity::builder().id(building_page_3_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building_page_3).send().await?.done()?;
    let page = broker
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .ids(vec![building_page_1_id.clone(), building_page_2_id.clone(), building_page_3_id.clone()])
        .limit(2)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.next(), none());
    expect_that!(page.prev(), none());
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.total(), some(eq(3)));
    let page_2 = broker
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .ids(vec![building_page_1_id.clone(), building_page_2_id.clone(), building_page_3_id.clone()])
        .limit(2)
        .offset(2)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_page_1_id.clone(), building_page_2_id.clone(), building_page_3_id.clone()])
        .limit(2)
        .offset(0)
        .send()
        .await?;
    expect_that!(page_3.next(), some(anything()));
    Ok(())
}
