//! An unknown attribute is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{AttributeName, Entity, ErrorType, Property, Relationship, Value};

/// Pins: Delete Attribute addressed to an Attribute the Entity does not have is refused with 404 `ResourceNotFound`
/// Clause: 5.6.5.4, 6.7.3.2
#[gtest]
#[tokio::test]
async fn n_att_del_0008_an_unknown_attribute_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let person_bob_id = namespace.urn("Person", "Bob")?;
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
            Relationship::builder(off_street_parking_downtown1_id)
                .observed_at("2017-07-29T12:00:04Z".parse()?)
                .relationship("providedBy", person_bob_id),
        )
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let answer = broker
        .with_responses()
        .entities()
        .delete_attribute(&vehicle_id, &"nonExistentAttr".parse::<AttributeName>()?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
