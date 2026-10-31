//! A percent encoded relationship object reads back as written.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityId, NgsiLdMediaType},
};

/// Pins: An Entity created with a Relationship whose object percent-encodes the colons after the scheme and a
/// Property whose value holds a literal percent sign is retrieved with both exactly as written
/// Clause: 4.5.3.2, 5.7.1; IETF RFC 3986 sections 2.2 and 6.2.2.2
#[gtest]
#[tokio::test]
async fn n_ent_ret_9020_a_percent_encoded_relationship_object_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_percent_id = namespace.urn("Device", "percent")?;
    let device_type = namespace.entity_type("Device")?;
    let ngsi_ld_3_abuilding_id = format!("urn:ngsi-ld%3ABuilding%3A{namespace}-percent-target").parse::<EntityId>()?;
    let device_percent = Entity::builder()
        .id(device_percent_id.clone())
        .entity_type(device_type)
        .relationship("ref", format!("urn:ngsi-ld%3ABuilding%3A{namespace}-percent-target").parse::<EntityId>()?)
        .property("ratio", "50%")
        .build()?;
    broker.entities().create(&device_percent).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_percent_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("ref", any_attribute().object(ngsi_ld_3_abuilding_id)));
    expect_that!(entity, has_attribute("ratio", any_attribute().value("50%")));
    Ok(())
}
