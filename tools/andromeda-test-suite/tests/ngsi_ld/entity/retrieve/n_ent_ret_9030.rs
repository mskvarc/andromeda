//! A value key named type holding a space reads back as written.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, value::object};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Value},
};

/// Pins: A Property value {"type": "Postal Address <ns>", "street": "x"} reads back with that type, spaces
/// included, and street x
/// Clause: 4.6.3, 4.6.4, Table 5.2.5-1
#[gtest]
#[tokio::test]
async fn n_ent_ret_9030_a_value_key_named_type_holding_a_space_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let shop_spaced_type_id = namespace.urn("Shop", "spaced-type")?;
    let shop_type = namespace.entity_type("Shop")?;
    let shop_spaced_type = Entity::builder()
        .id(shop_spaced_type_id.clone())
        .entity_type(shop_type)
        .property(
            "p",
            object([("type", Value::from(format!("Postal Address {namespace}"))), ("street", Value::from("x"))]),
        )
        .build()?;
    broker.entities().create(&shop_spaced_type).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&shop_spaced_type_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "p",
            any_attribute()
                .value_member(["type"], format!("Postal Address {namespace}"))
                .value_member(["street"], "x")
        )
    );
    Ok(())
}
