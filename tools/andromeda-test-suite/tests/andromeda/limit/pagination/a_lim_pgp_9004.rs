//! A broker without cursor pagination refuses a cursor.

use andromeda_test_suite::{broker, config::BrokerRole, namespace::Namespace, outcome::Done, page_link::carries, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: On a broker with cursor pagination off and an offset default, Query Entities with an empty cursor answers
/// 400 `InvalidRequest`, and the same query without it answers a next pointer carrying offset=2
/// Clause: 5.5.9, 6.3.10, 6.3.2
/// Recorded: Andromeda pages a fresh query by offset by default and walks by an opaque cursor once it sends an empty
/// `cursor`; `[extensions.cursor_pagination]` `fresh_requests` = "cursor" makes a fresh query walk by cursor, and
/// `mode` = "disabled" withdraws the cursor parameter
#[gtest]
#[tokio::test]
async fn a_lim_pgp_9004_a_broker_without_cursor_pagination_refuses_a_cursor() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_prefixed = broker::client(BrokerRole::Prefixed)?;
    let policy_item_0000_id = namespace.urn("PolicyItem", "0000")?;
    let policy_item_type = namespace.entity_type("PolicyItem")?;
    let policy_item_0001_id = namespace.urn("PolicyItem", "0001")?;
    let policy_item_0002_id = namespace.urn("PolicyItem", "0002")?;
    let entities = [
        Entity::builder()
            .id(policy_item_0000_id)
            .entity_type(policy_item_type.clone())
            .property("name", "p0")
            .build()?,
        Entity::builder()
            .id(policy_item_0001_id)
            .entity_type(policy_item_type.clone())
            .property("name", "p1")
            .build()?,
        Entity::builder()
            .id(policy_item_0002_id)
            .entity_type(policy_item_type.clone())
            .property("name", "p2")
            .build()?,
    ];
    broker_prefixed.entities().batch_create(&entities).send().await?.done()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker_prefixed.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=PolicyItem_{namespace}&limit=2&cursor="),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker_prefixed.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let page = broker_prefixed
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&policy_item_type)
        .limit(2)
        .send()
        .await?;
    expect_that!(page.next(), some(carries("offset", "2")));
    expect_that!(page.items(), len(eq(2)));
    Ok(())
}
