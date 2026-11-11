//! The same query posted twice points at the same page.

use andromeda_test_suite::{broker, namespace::Namespace, paged_buildings::PagedBuildings};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::NgsiLdMediaType,
    query::{Locality, q},
};

/// Pins: Query Entities through POST `/entityOperations/query` with the same body and parameters twice answers the
/// same next Link both times, and a body differing in `q` answers another
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10, 6.23.3.1
#[gtest]
#[tokio::test]
async fn a_qry_pag_9091_the_same_query_posted_twice_points_at_the_same_page() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let buildings = PagedBuildings::create(&broker, &namespace).await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let mut nexts = Vec::new();
    for threshold in [5, 5, 15] {
        let page = json
            .entities()
            .query()
            .method(QueryMethod::Post)
            .entity_type(&buildings.entity_type)
            .q(q::attr("height").gt(threshold))
            .locality(Locality::Local)
            .limit(2)
            .send()
            .await?;
        nexts.push(page.next().or_fail()?.clone());
    }
    expect_that!(&nexts[0], eq(&nexts[1]));
    expect_that!(&nexts[0], not(eq(&nexts[2])));
    Ok(())
}
