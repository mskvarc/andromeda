//! A cursor followed with the same q written in another order is admitted.

use andromeda_test_suite::{
    broker,
    followed_link::request_replacing,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    paged_buildings::PagedBuildings,
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{
        ResponseHead,
        headers::{Link, RelationType},
    },
    query::q,
};

/// Pins: Query Entities following a next pointer with the terms of its `q` conjunction written in another order
/// answers the following page
/// Clause: 4.9, 5.5.9.1, 6.3.10
#[gtest]
#[tokio::test]
async fn a_qry_pag_9096_a_cursor_followed_with_the_same_q_in_another_order_is_admitted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let buildings = PagedBuildings::create(&broker, &namespace).await?;
    let filter = q::attr("height").gt(5).and(q::attr("name").ne("none")).build()?.to_string();
    let first = buildings.first_page(&broker, "2", &[("q", filter.as_str())]).await?;
    expect_that!(first.status(), eq(StatusCode::OK));
    expect_that!(entities(&first)?.as_slice(), ids_in_order(&buildings.ids[..2]));
    let link = ResponseHead::new(first.headers().clone()).typed::<Link>().valid().cloned().or_fail()?;
    let reordered_filter = q::attr("name").ne("none").and(q::attr("height").gt(5)).build()?.to_string();
    let reordered =
        request_replacing(&broker, &link, &RelationType::Next, "q", &reordered_filter)?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let page = broker.send_unchecked(reordered).await?;
    expect_that!(page.status(), eq(StatusCode::OK));
    expect_that!(entities(&page)?.as_slice(), ids_in_order(&buildings.ids[2..]));
    Ok(())
}
