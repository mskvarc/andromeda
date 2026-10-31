//! A null fragment deleting the last instance removes the attribute.

use andromeda_test_suite::{broker, entity_matcher::lacks_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, AttributeName, Change, Entity, InstanceDeletion, NgsiLdMediaType, Property},
};

/// Pins: Partial Attribute Update with a null value and the `datasetId` of the only instance answers 204 and
/// removes the attribute
/// Clause: 5.5.8, 5.6.5, 5.6.4.1
/// Recorded: 5.6.4.1 says Partial Attribute update does not delete the whole Attribute, while 5.5.8 routes instance
/// deletion to 5.6.5, which removes the Attribute with its last instance; Andromeda follows 5.5.8
#[gtest]
#[tokio::test]
async fn o_att_pat_9015_a_null_fragment_deleting_the_last_instance_removes_the_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_last_instance_id = namespace.urn("Building", "last-instance")?;
    let building_type = namespace.entity_type("Building")?;
    let dataset_01 = namespace.dataset_id("Dataset", "01")?;
    let building_last_instance = Entity::builder()
        .id(building_last_instance_id.clone())
        .entity_type(building_type)
        .property("street", Property::builder(4).dataset_id(namespace.dataset_id("Dataset", "01")?))
        .build()?;
    broker.entities().create(&building_last_instance).send().await?.done()?;
    let deletion = Change::Delete(InstanceDeletion::new(AttributeKind::Property).with_dataset_id(dataset_01));
    broker
        .entities()
        .update_attribute(&building_last_instance_id, &"street".parse::<AttributeName>()?, deletion)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_last_instance_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("street"));
    Ok(())
}
