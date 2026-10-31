//! A percent encoded target joins only the entity spelled the same.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityId, NgsiLdMediaType},
};

/// Pins: Retrieve Entity with join flat of a Room whose Relationship percent-encodes the colons of its object
/// appends the Building whose identifier is spelled the same way, and not the Building whose identifier is the
/// decoded spelling
/// Clause: 4.5.23.1, 4.5.23.3, 5.7.1.4; IETF RFC 3986 sections 2.2 and 6.2.2.2
#[gtest]
#[tokio::test]
async fn n_lnk_ret_9012_a_percent_encoded_target_joins_only_the_entity_spelled_the_same() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let room_r73_id = namespace.urn("Room", "r73")?;
    let room_type = namespace.entity_type("Room")?;
    let ngsi_ld_building_id = format!("urn:ngsi-ld:Building%3A{namespace}-1").parse::<EntityId>()?;
    let ngsi_ld_building_id_2 = format!("urn:ngsi-ld:Building%3A{namespace}-2").parse::<EntityId>()?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "decoded")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let entity = Entity::builder()
        .id(format!("urn:ngsi-ld:Building%3A{namespace}-2").parse::<EntityId>()?)
        .entity_type(building_type.clone())
        .property("name", "encoded")
        .build()?;
    broker.entities().create(&entity).send().await?.done()?;
    let room_r73 = Entity::builder()
        .id(room_r73_id.clone())
        .entity_type(room_type)
        .relationship("locatedIn", format!("urn:ngsi-ld:Building%3A{namespace}-1").parse::<EntityId>()?)
        .relationship("servedBy", format!("urn:ngsi-ld:Building%3A{namespace}-2").parse::<EntityId>()?)
        .build()?;
    broker.entities().create(&room_r73).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&room_r73_id)
        .send()
        .await?;
    let entities = retrieved.body();
    expect_that!(entities.as_slice(), contains(has_id(&ngsi_ld_building_id_2)));
    expect_that!(entities.as_slice(), not(contains(has_id(&building_id))));
    expect_that!(
        entities.as_slice(),
        nth(
            0,
            all!(has_id(&room_r73_id), has_attribute("locatedIn", any_attribute().object(ngsi_ld_building_id)))
        )
    );
    Ok(())
}
