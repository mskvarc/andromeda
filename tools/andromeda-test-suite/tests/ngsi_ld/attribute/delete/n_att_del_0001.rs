//! Without a dataset id deletes only the default instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, NgsiLdMediaType, Property, Value},
};

/// Pins: Delete Attribute with neither `datasetId` nor `deleteAll` answers 204 and removes the default instance
/// only; both instances with a `datasetId` stay
/// Clause: 5.6.5.4, 6.7.3.2
#[gtest]
#[tokio::test]
async fn n_att_del_0001_without_a_dataset_id_deletes_only_the_default_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let speedometer_dataset = namespace.dataset_id("Property", "speedometer")?;
    let gps_dataset = namespace.dataset_id("Property", "gps")?;
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
    expect_that!(entity, has_instances("speed", 2));
    expect_that!(
        entity,
        has_attribute("speed", any_attribute().dataset_id(speedometer_dataset).value(Value::Number("54.5".parse()?)))
    );
    expect_that!(entity, has_attribute("speed", any_attribute().dataset_id(gps_dataset).value(56)));
    Ok(())
}
