//! A cursor followed with a q of another value is refused.

use andromeda_test_suite::{broker, followed_link::request_replacing, namespace::Namespace, paged_buildings::PagedBuildings, problem::problem};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{
        ResponseHead,
        headers::{Link, RelationType},
    },
    model::ErrorType,
    query::q,
};

/// Pins: Query Entities following a next pointer with one term of its `q` conjunction given another value answers 400
/// `BadRequestData`, also when the terms are written in another order
/// Clause: 4.9, 5.5.9.1, 6.3.20
#[gtest]
#[tokio::test]
async fn a_qry_pag_9097_a_cursor_followed_with_a_q_of_another_value_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let buildings = PagedBuildings::create(&broker, &namespace).await?;
    let filter = q::attr("height").gt(5).and(q::attr("name").ne("none")).build()?.to_string();
    let first = buildings.first_page(&broker, "2", &[("q", filter.as_str())]).await?;
    expect_that!(first.status(), eq(StatusCode::OK));
    let link = ResponseHead::new(first.headers().clone()).typed::<Link>().valid().cloned().or_fail()?;
    let changed = [
        q::attr("height").gt(25).and(q::attr("name").ne("none")).build()?.to_string(),
        q::attr("name").ne("none").and(q::attr("height").gt(25)).build()?.to_string(),
    ];
    for changed in &changed {
        let replayed =
            request_replacing(&broker, &link, &RelationType::Next, "q", changed)?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
        let answer = broker.send_unchecked(replayed).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    Ok(())
}
