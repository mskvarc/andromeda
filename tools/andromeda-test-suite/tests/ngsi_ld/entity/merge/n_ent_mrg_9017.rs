//! A merge keeps a value key holding a space.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, value::object};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, Value},
};

/// Pins: Merge Entity of {"opening hours": "10-6"} into a value holding opening hours and closed on answers 204 and
/// leaves opening hours 10-6 beside closed on Sunday
/// Clause: 4.6.4, 5.5.12, 5.6.17.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9017_a_merge_keeps_a_value_key_holding_a_space() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let shop_merged_id = namespace.urn("Shop", "merged")?;
    let shop_type = namespace.entity_type("Shop")?;
    let shop_merged = Entity::builder()
        .id(shop_merged_id.clone())
        .entity_type(shop_type)
        .property("p", object([("opening hours", Value::from("9-5")), ("closed on", Value::from("Sunday"))]))
        .build()?;
    broker.entities().create(&shop_merged).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("p", object([("opening hours", Value::from("10-6"))]))
        .build()?;
    broker.entities().merge(&shop_merged_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&shop_merged_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "p",
            any_attribute().value_member(["opening hours"], "10-6").value_member(["closed on"], "Sunday")
        )
    );
    Ok(())
}
