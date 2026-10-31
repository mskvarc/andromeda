//! A cursor without its query digest is refused.

use andromeda_test_suite::{
    broker,
    followed_link::request_without,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    paged_buildings::PagedBuildings,
    problem::problem,
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{
        ResponseHead,
        headers::{Link, RelationType},
    },
    model::ErrorType,
};

/// Pins: Query Entities with a cursor but no `pageQuery` answers 400 `BadRequestData`, every cursor pointer carrying one
/// Clause: 5.5.9.1, 6.3.20
#[gtest]
#[tokio::test]
async fn a_qry_pag_9087_a_cursor_without_its_query_digest_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let buildings = PagedBuildings::create(&broker, &namespace).await?;
    let first = buildings.first_page(&broker, "2", &[("q", "height>5")]).await?;
    expect_that!(first.status(), eq(StatusCode::OK));
    expect_that!(entities(&first)?.as_slice(), ids_in_order(&buildings.ids[..2]));
    let link = ResponseHead::new(first.headers().clone()).typed::<Link>().valid().cloned().or_fail()?;
    let replayed = request_without(&broker, &link, &RelationType::Next, "pageQuery")?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer = broker.send_unchecked(replayed).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
