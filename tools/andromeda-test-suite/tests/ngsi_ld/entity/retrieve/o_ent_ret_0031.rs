//! Types read back in written order.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Retrieve Entity answers the types an entity was created with in the order they were written, not in an
/// order of their names
/// Clause: 4.16, 5.7.1.4
/// Recorded: 4.16 makes an entity's types a set and fixes no order among them; Andromeda answers them in the order
/// they were written
#[gtest]
#[tokio::test]
async fn o_ent_ret_0031_types_read_back_in_written_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let zebra_type = namespace.entity_type("Zebra")?;
    let aardvark_type = namespace.entity_type("Aardvark")?;
    let moose_type = namespace.entity_type("Moose")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(vec![zebra_type.clone(), aardvark_type.clone(), moose_type.clone()])
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity.entity_type.as_slice(),
        elements_are![eq(&zebra_type), eq(&aardvark_type), eq(&moose_type)]
    );
    Ok(())
}
