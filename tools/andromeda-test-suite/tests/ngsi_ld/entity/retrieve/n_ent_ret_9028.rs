//! A value key holding a space reads back as written.

use andromeda_test_suite::{attribute_matcher::property, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, value::object};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Value},
};

/// Pins: A Property value whose keys hold spaces at every depth, inside an array included, reads back with those
/// keys as written
/// Clause: 4.6.3, 4.6.4, Table 5.2.5-1
#[gtest]
#[tokio::test]
async fn n_ent_ret_9028_a_value_key_holding_a_space_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let shop_spaced_key_id = namespace.urn("Shop", "spaced-key")?;
    let shop_type = namespace.entity_type("Shop")?;
    let shop_spaced_key = Entity::builder()
        .id(shop_spaced_key_id.clone())
        .entity_type(shop_type)
        .property(
            "p",
            object([
                ("opening hours", Value::from("9-5")),
                (
                    "nested",
                    object([("a b", Value::Array(vec![object([("c d", Value::from(1))]), object([("e f", Value::from(2))])]))]),
                ),
            ]),
        )
        .build()?;
    broker.entities().create(&shop_spaced_key).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&shop_spaced_key_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "p",
            property()
                .value_member(["opening hours"], "9-5")
                .length_at(["nested", "a b"], 2)
                .value_member(["nested", "a b", "0", "c d"], 1)
                .value_member(["nested", "a b", "1", "e f"], 2)
        )
    );
    Ok(())
}
