//! A percent encoded space in an id and a relationship object reads back verbatim.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityId, NgsiLdMediaType},
};

/// Pins: Create Entity with the id urn:ngsi-ld:Building:<ns>-has%20space and the Relationship object
/// urn:ngsi-ld:City:<ns>-has%20space answers 201, and Retrieve Entity reads both back with %20 as written
/// Clause: 4.5.3.2, 5.2.4, 5.7.1; IETF RFC 3986 sections 2.1, 2.4 and 6.2.2.2
#[gtest]
#[tokio::test]
async fn n_cmn_val_9029_a_percent_encoded_space_in_an_id_and_a_relationship_object_reads_back_verbatim() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let ngsi_ld_building_id = format!("urn:ngsi-ld:Building:{namespace}-has%20space").parse::<EntityId>()?;
    let ngsi_ld_city_id = format!("urn:ngsi-ld:City:{namespace}-has%20space").parse::<EntityId>()?;
    let entity = Entity::builder()
        .id(format!("urn:ngsi-ld:Building:{namespace}-has%20space").parse::<EntityId>()?)
        .entity_type(building_type)
        .relationship("locatedAt", format!("urn:ngsi-ld:City:{namespace}-has%20space").parse::<EntityId>()?)
        .build()?;
    broker.entities().create(&entity).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&ngsi_ld_building_id)
        .send()
        .await?;
    let entity_2 = retrieved.body();
    expect_that!(entity_2.id, eq(&ngsi_ld_building_id));
    expect_that!(entity_2, has_attribute("locatedAt", any_attribute().object(ngsi_ld_city_id)));
    broker.entities().delete(&ngsi_ld_building_id).send().await?.done()?;
    Ok(())
}
