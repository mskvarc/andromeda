//! The page links of a posted query read the held query by its key.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    page_link::{carries, carries_parameter},
    paged_buildings::PagedBuildings,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{QueryMethod, ResultsCount},
    },
    model::NgsiLdMediaType,
    query::{Locality, q},
};

/// Pins: Query Entities through POST `/entityOperations/query` with `type`, `q`, limit 2, count and local answers a
/// next Link that is a GET on `/entities` carrying `postedQuery`, limit 2, a cursor, count and local and none of
/// `type`, `q`, `pageQuery` and `pageContext`; the prev Link of the next page is a GET on `/entities` naming the same
/// `postedQuery`
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10, 6.23.3.1
#[gtest]
#[tokio::test]
async fn a_qry_pag_9090_the_page_links_of_a_posted_query_read_the_held_query_by_its_key() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let buildings = PagedBuildings::create(&broker, &namespace).await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let first = json
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&buildings.entity_type)
        .q(q::attr("height").gt(5))
        .locality(Locality::Local)
        .limit(2)
        .count(ResultsCount::Include)
        .send()
        .await?;
    let next = first.next().or_fail()?;
    expect_that!(next.path(), ends_with("/ngsi-ld/v1/entities"));
    expect_that!(next, carries_parameter("postedQuery"));
    expect_that!(next, carries("limit", "2"));
    expect_that!(next, carries_parameter("cursor"));
    expect_that!(next, carries("count", "true"));
    expect_that!(next, carries("local", "true"));
    for member in ["type", "q", "pageQuery", "pageContext"] {
        expect_that!(next, not(carries_parameter(member)));
    }
    let key = next
        .query_pairs()
        .find(|(name, _)| name == "postedQuery")
        .map(|(_, value)| value.into_owned())
        .or_fail()?;
    let second = json.send(&first.next_page().or_fail()?).await?;
    let prev = second.prev().or_fail()?;
    expect_that!(prev.path(), ends_with("/ngsi-ld/v1/entities"));
    expect_that!(prev, carries("postedQuery", &key));
    expect_that!(prev, not(carries_parameter("type")));
    Ok(())
}
