//! A relationship equality tells a percent encoded object from its decoded spelling.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityId, NgsiLdMediaType, Uri},
    query::q,
};

/// Pins: Query Entities with q equal to a Relationship's percent-encoded object selects the Entity, and with q
/// equal to the decoded spelling of that object does not
/// Clause: 4.9, 4.5.3.2; IETF RFC 3986 sections 2.2 and 6.2.2.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9056_a_relationship_equality_tells_a_percent_encoded_object_from_its_decoded_spelling() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let room_encoded_id = namespace.urn("Room", "encoded")?;
    let room_type = namespace.entity_type("Room")?;
    let room_encoded = Entity::builder()
        .id(room_encoded_id)
        .entity_type(room_type.clone())
        .relationship("locatedIn", format!("urn:ngsi-ld%3ABuilding%3A{namespace}-1").parse::<EntityId>()?)
        .build()?;
    broker.entities().create(&room_encoded).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&room_type)
        .q(q::attr("locatedIn").eq(format!("urn:ngsi-ld%3ABuilding%3A{namespace}-1").as_str()))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&room_type)
        .q(q::attr("locatedIn").eq(format!("urn:ngsi-ld:Building:{namespace}-1").as_str()))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&room_type)
        .q(q::attr("locatedIn").eq(q::UnquotedUri::try_from(format!("urn:ngsi-ld:Building:{namespace}-1").parse::<Uri>()?)?))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    Ok(())
}
