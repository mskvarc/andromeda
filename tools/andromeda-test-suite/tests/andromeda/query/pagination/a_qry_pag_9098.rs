//! A cursor followed with another limit or an added count is admitted.

use andromeda_test_suite::{
    broker,
    followed_link::{request_replacing, request_with},
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

/// Pins: Query Entities following a next pointer with another `limit` answers the following page of that size, and
/// one with `count` added answers the following page
/// Clause: 4.9, 5.5.9.1, 6.3.10
#[gtest]
#[tokio::test]
async fn a_qry_pag_9098_a_cursor_followed_with_another_limit_is_admitted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let buildings = PagedBuildings::create(&broker, &namespace).await?;
    let filter = q::attr("height").gt(5).and(q::attr("name").ne("none")).build()?.to_string();
    let first = buildings.first_page(&broker, "2", &[("q", filter.as_str())]).await?;
    expect_that!(first.status(), eq(StatusCode::OK));
    let link = ResponseHead::new(first.headers().clone()).typed::<Link>().valid().cloned().or_fail()?;
    let narrower = request_replacing(&broker, &link, &RelationType::Next, "limit", "1")?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let page = broker.send_unchecked(narrower).await?;
    expect_that!(page.status(), eq(StatusCode::OK));
    expect_that!(entities(&page)?.as_slice(), ids_in_order(&buildings.ids[2..3]));
    let respelled =
        request_with(&broker, &link, &RelationType::Next, &[("count", "true")])?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let counted = broker.send_unchecked(respelled).await?;
    expect_that!(counted.status(), eq(StatusCode::OK));
    expect_that!(entities(&counted)?.as_slice(), ids_in_order(&buildings.ids[2..]));
    Ok(())
}
