//! A cursor followed with another identifier list is refused.

use andromeda_test_suite::{
    broker,
    followed_link::request_replacing,
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

/// Pins: Query Entities following a next pointer with another `id` list answers 400 `BadRequestData`, and with the
/// same identifiers listed in another order answers the following page
/// Clause: 5.5.9.1, 6.3.10, 6.3.20
#[gtest]
#[tokio::test]
async fn a_qry_pag_9099_a_cursor_followed_with_another_identifier_list_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let buildings = PagedBuildings::create(&broker, &namespace).await?;
    let listed = |ids: &[usize]| ids.iter().map(|index| buildings.ids[*index].to_string()).collect::<Vec<_>>().join(",");
    let first = buildings.first_page(&broker, "2", &[("id", listed(&[0, 1, 2]).as_str())]).await?;
    expect_that!(first.status(), eq(StatusCode::OK));
    expect_that!(entities(&first)?.as_slice(), ids_in_order(&buildings.ids[..2]));
    let link = ResponseHead::new(first.headers().clone()).typed::<Link>().valid().cloned().or_fail()?;
    let wider = request_replacing(&broker, &link, &RelationType::Next, "id", &listed(&[0, 1, 2, 3]))?
        .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let refused = broker.send_unchecked(wider).await;
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let reordered = request_replacing(&broker, &link, &RelationType::Next, "id", &listed(&[2, 0, 1]))?
        .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let page = broker.send_unchecked(reordered).await?;
    expect_that!(page.status(), eq(StatusCode::OK));
    expect_that!(entities(&page)?.as_slice(), ids_in_order(&buildings.ids[2..3]));
    Ok(())
}
