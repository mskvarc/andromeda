//! An offset default broker hands out offset links.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    page_link::{carries, carries_parameter, links},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: On a broker whose default pagination is offset, Query Entities with limit 2 over three Entities answers
/// the first two with a next pointer carrying offset=2 and no cursor
/// Clause: 5.5.9, 6.3.10
/// Recorded: Andromeda pages by an opaque cursor by default; `[pagination]` `default_mode` = "offset" makes a fresh
/// query page by offset, and `[extensions]` `cursor_pagination` = "disabled" withdraws the cursor parameter
#[gtest]
#[tokio::test]
async fn a_lim_pgp_9002_an_offset_default_broker_hands_out_offset_links() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_edge = broker::client(BrokerRole::Edge)?;
    let policy_item_0000_id = namespace.urn("PolicyItem", "0000")?;
    let policy_item_type = namespace.entity_type("PolicyItem")?;
    let policy_item_0001_id = namespace.urn("PolicyItem", "0001")?;
    let policy_item_0002_id = namespace.urn("PolicyItem", "0002")?;
    let entities = [
        Entity::builder()
            .id(policy_item_0000_id.clone())
            .entity_type(policy_item_type.clone())
            .property("name", "p0")
            .build()?,
        Entity::builder()
            .id(policy_item_0001_id.clone())
            .entity_type(policy_item_type.clone())
            .property("name", "p1")
            .build()?,
        Entity::builder()
            .id(policy_item_0002_id)
            .entity_type(policy_item_type.clone())
            .property("name", "p2")
            .build()?,
    ];
    broker_edge.entities().batch_create(&entities).send().await?.done()?;
    let page = broker_edge
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&policy_item_type)
        .limit(2)
        .send()
        .await?;
    expect_that!(page.next(), some(carries("offset", "2")));
    expect_that!(links(&page), each(not(carries_parameter("cursor"))));
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&policy_item_0000_id)));
    expect_that!(page.items(), nth(1, has_id(&policy_item_0001_id)));
    Ok(())
}
