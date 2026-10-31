//! A null with a fragment dataset id deletes only that instance.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, AttributeName, Change, Entity, InstanceDeletion, NgsiLdMediaType, Property, Value},
};

/// Pins: Partial Attribute Update with a Fragment whose value is NGSI-LD Null and whose `datasetId` names one of
/// two instances answers 204 and deletes that instance only
/// Clause: 5.6.4.4, 5.5.8, 5.6.5.4
/// Recorded: clause 5.6.4.4 says NGSI-LD Nulls "should" be supported by Partial Attribute Update
/// (`OperationNotSupported` otherwise); Andromeda supports them, and clause 5.5.8 then deletes the instance with
/// that `datasetId` as clause 5.6.5 does
#[gtest]
#[tokio::test]
async fn o_att_pat_0030_a_null_with_a_fragment_dataset_id_deletes_only_that_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let gps_dataset = namespace.dataset_id("Property", "gps")?;
    let speedometer_dataset = namespace.dataset_id("Property", "speedometer")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(55).dataset_id(namespace.dataset_id("Property", "gps")?))
        .property(
            "speed",
            Property::builder(Value::Number("54.5".parse()?)).dataset_id(namespace.dataset_id("Property", "speedometer")?),
        )
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let deletion = Change::Delete(InstanceDeletion::new(AttributeKind::Property).with_dataset_id(gps_dataset));
    broker
        .entities()
        .update_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?, deletion)
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
    expect_that!(
        entity,
        has_attribute(
            "speed",
            any_attribute().with_dataset_id(speedometer_dataset).value(Value::Number("54.5".parse()?))
        )
    );
    Ok(())
}
