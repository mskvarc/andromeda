//! An offset default broker walks by cursor after an empty cursor.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    identity_matcher::ids_in_order,
    link_header::link_text,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    unchecked_page::{entities, follow},
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{http::headers::RelationType, model::Entity};

/// Pins: On a broker whose default pagination is offset, Query Entities with limit 2 and an empty cursor over five
/// Entities answers cursor pointers and no offset, and following next walks the five once in identifier order
/// Clause: 5.5.9.1, 6.3.10
/// Recorded: Andromeda pages by an opaque cursor by default; `[pagination]` `default_mode` = "offset" makes a fresh
/// query page by offset, and `[extensions]` `cursor_pagination` = "disabled" withdraws the cursor parameter
#[gtest]
#[tokio::test]
async fn a_lim_pgp_9003_an_offset_default_broker_walks_by_cursor_after_an_empty_cursor() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_edge = broker::client(BrokerRole::Edge)?;
    let policy_item_type = namespace.entity_type("PolicyItem")?;
    let mut policy_item_ids = Vec::new();
    let mut policy_items = Vec::new();
    for (index, name) in ["p0", "p1", "p2", "p3", "p4"].into_iter().enumerate() {
        let policy_item_id = namespace.urn("PolicyItem", format!("{index:04}"))?;
        policy_items.push(
            Entity::builder()
                .id(policy_item_id.clone())
                .entity_type(policy_item_type.clone())
                .property("name", name)
                .build()?,
        );
        policy_item_ids.push(policy_item_id);
    }
    broker_edge.entities().batch_create(&policy_items).send().await?.done()?;
    // The parameter model does not know the cursor, so the first page is requested as written.
    let request = request_to(&broker_edge, Method::GET, &format!("entities?type=PolicyItem_{namespace}&limit=2&cursor="))?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let first = broker_edge.send_unchecked(request).await;
    expect_that!(&first, answered(StatusCode::OK));
    let first_response = first.as_ref().or_fail()?;
    expect_that!(link_text(first_response.headers()), contains_substring("cursor="));
    expect_that!(link_text(first_response.headers()), not(contains_substring("offset=")));
    expect_that!(entities(first_response)?.as_slice(), ids_in_order(&policy_item_ids[0..2]));
    // A page link carries the cursor, which the typed page request does not follow from an unchecked answer.
    let second = follow(&broker_edge, first_response, &RelationType::Next).await?.or_fail()?;
    expect_that!(&second, answered(StatusCode::OK));
    let second_response = second.as_ref().or_fail()?;
    expect_that!(link_text(second_response.headers()), not(contains_substring("offset=")));
    expect_that!(entities(second_response)?.as_slice(), ids_in_order(&policy_item_ids[2..4]));
    let third = follow(&broker_edge, second_response, &RelationType::Next).await?.or_fail()?;
    expect_that!(&third, answered(StatusCode::OK));
    let third_response = third.as_ref().or_fail()?;
    expect_that!(link_text(third_response.headers()), not(contains_substring("rel=\"next\"")));
    expect_that!(entities(third_response)?.as_slice(), ids_in_order(&policy_item_ids[4..5]));
    Ok(())
}
