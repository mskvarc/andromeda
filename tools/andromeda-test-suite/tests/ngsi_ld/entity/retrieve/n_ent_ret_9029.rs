//! A value key named id reads back as written.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, value::object};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Value},
};

/// Pins: A Property value {"id": "abc-<ns>", "name": "x"}, whose id the core context aliases to @id and which is no
/// absolute IRI, reads back with that id and name x
/// Clause: 4.6.3, 4.6.4, Table 5.2.5-1
#[gtest]
#[tokio::test]
async fn n_ent_ret_9029_a_value_key_named_id_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let shop_relative_id_id = namespace.urn("Shop", "relative-id")?;
    let shop_type = namespace.entity_type("Shop")?;
    let shop_relative_id = Entity::builder()
        .id(shop_relative_id_id.clone())
        .entity_type(shop_type)
        .property("p", object([("id", Value::from(format!("abc-{namespace}"))), ("name", Value::from("x"))]))
        .build()?;
    broker.entities().create(&shop_relative_id).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&shop_relative_id_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "p",
            any_attribute().value_member(["id"], format!("abc-{namespace}")).value_member(["name"], "x")
        )
    );
    Ok(())
}
