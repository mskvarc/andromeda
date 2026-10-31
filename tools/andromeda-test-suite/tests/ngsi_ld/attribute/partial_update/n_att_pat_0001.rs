//! A sub relationship takes the new object.

use andromeda_test_suite::{attribute_matcher::relationship, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeMembers, AttributeName, Entity, NgsiLdMediaType, Property, Relationship, Value},
};

/// Pins: Partial Attribute Update with a Fragment holding only a sub-Relationship answers 204, replaces that
/// sub-Relationship and leaves the rest of the Attribute alone
/// Clause: 5.6.4.4, 5.5.8, 6.7.3.1
#[gtest]
#[tokio::test]
async fn n_att_pat_0001_a_sub_relationship_takes_the_new_object() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let person_bob_id = namespace.urn("Person", "Bob")?;
    let person_john_doe_id = namespace.urn("Person", "JohnDoe")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(55).dataset_id(namespace.dataset_id("Property", "gps")?))
        .property(
            "speed",
            Property::builder(Value::Number("54.5".parse()?)).dataset_id(namespace.dataset_id("Property", "speedometer")?),
        )
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown1_id.clone())
                .observed_at("2017-07-29T12:00:04Z".parse()?)
                .relationship("providedBy", person_bob_id),
        )
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = AttributeMembers::builder().relationship("providedBy", person_john_doe_id.clone()).build()?;
    let answer = broker
        .with_responses()
        .entities()
        .update_attribute(&vehicle_id, &"isParked".parse::<AttributeName>()?, fragment)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
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
            "isParked",
            relationship()
                .object(off_street_parking_downtown1_id)
                .observed_at("2017-07-29T12:00:04Z".parse()?)
                .sub_attribute("providedBy", relationship().object(person_john_doe_id))
        )
    );
    Ok(())
}
