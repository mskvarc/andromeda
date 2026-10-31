//! A cursor followed with another limit or representation is admitted.

use andromeda_test_suite::{
    broker,
    followed_link::{request_replacing, request_with},
    identity_matcher::ids_in_order,
    link_header::link_text,
    namespace::Namespace,
    paged_buildings::PagedBuildings,
    unchecked_outcome::read_as,
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{
        ResponseHead,
        headers::{Link, RelationType},
        operation::{QueryEntities, Simplified},
    },
    model::{AttributeName, SimplifiedValue, Value},
};

/// Pins: Query Entities following a next pointer with another `limit`, or with `options` and `count` added, answers
/// the following page
/// Clause: 5.5.9.1, 6.3.10
#[gtest]
#[tokio::test]
async fn a_qry_pag_9088_a_cursor_followed_with_another_limit_or_representation_is_admitted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let buildings = PagedBuildings::create(&broker, &namespace).await?;
    let first = buildings.first_page(&broker, "2", &[("q", "height>5")]).await?;
    expect_that!(first.status(), eq(StatusCode::OK));
    let links = link_text(first.headers());
    expect_that!(&links, contains_substring("pageQuery="));
    expect_that!(&links, not(contains_substring("offset=")));
    let link = ResponseHead::new(first.headers().clone()).typed::<Link>().valid().cloned().or_fail()?;
    let narrower = request_replacing(&broker, &link, &RelationType::Next, "limit", "1")?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let page = broker.send_unchecked(narrower).await?;
    expect_that!(page.status(), eq(StatusCode::OK));
    expect_that!(entities(&page)?.as_slice(), ids_in_order(&buildings.ids[2..3]));
    let reshaped = request_with(&broker, &link, &RelationType::Next, &[("options", "keyValues"), ("count", "true")])?
        .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let sent = reshaped.as_request().clone();
    let answer = broker.send_unchecked(reshaped).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    // `keyValues` spells the simplified representation (clause 4.5.3), so a query of simplified entities reads it.
    let page_2 = read_as(&QueryEntities::builder().format(Simplified).build(), &sent, answer)?;
    expect_that!(page_2.items(), ids_in_order(&buildings.ids[2..]));
    expect_that!(
        page_2
            .items()
            .first()
            .and_then(|building| building.attributes.get(&"height".parse::<AttributeName>().ok()?)),
        some(eq(&SimplifiedValue::Value(Value::from(30))))
    );
    expect_that!(page_2.total(), some(eq(4)));
    Ok(())
}
