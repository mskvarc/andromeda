//! A posted query whose result fits one page points at no other page.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Query Entities through POST `/entityOperations/query` with limit 3 over two Entities answers both and
/// neither a next nor a prev Link
/// Clause: 5.5.9.1, 6.3.10, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_pag_9019_a_posted_query_whose_result_fits_one_page_points_at_no_other_page() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let ids = vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?];
    for id in &ids {
        let building = Entity::builder().id(id.clone()).entity_type(building_type.clone()).build()?;
        broker.entities().create(&building).send().await?.done()?;
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .ids(ids.clone())
        .limit(3)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.next(), none());
    expect_that!(page.prev(), none());
    Ok(())
}
