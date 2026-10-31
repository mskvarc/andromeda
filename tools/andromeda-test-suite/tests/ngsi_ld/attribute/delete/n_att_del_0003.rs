//! Delete all removes every instance.

use andromeda_test_suite::{broker, entity_matcher::lacks_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::InstanceSelection},
    model::{AttributeName, Entity, NgsiLdMediaType, Property, Value},
};

/// Pins: Delete Attribute with `deleteAll=true` answers 204 and removes the default instance and every instance
/// with a `datasetId`
/// Clause: 5.6.5.4, 6.7.3.2
#[gtest]
#[tokio::test]
async fn n_att_del_0003_delete_all_removes_every_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 55)
        .property(
            "speed",
            Property::builder(Value::Number("54.5".parse()?)).dataset_id(namespace.dataset_id("Property", "speedometer")?),
        )
        .property("speed", Property::builder(56).dataset_id(namespace.dataset_id("Property", "gps")?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    broker
        .entities()
        .delete_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?)
        .instances(InstanceSelection::All)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&vehicle_id));
    expect_that!(entity, lacks_attribute("speed"));
    Ok(())
}
