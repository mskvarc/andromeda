//! An unknown dataset id is resource not found.

use andromeda_test_suite::{broker, entity_matcher::has_instances, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::InstanceSelection},
    model::{AttributeName, Entity, ErrorType, NgsiLdMediaType, Property, Relationship, Value},
};

/// Pins: Delete Attribute with a `datasetId` that no instance of the Attribute has is refused with 404
/// `ResourceNotFound` and both instances stay
/// Clause: 5.6.5.4, 6.7.3.2
/// Recorded: clause 5.6.5.4 raises `ResourceNotFound` when the Entity does not contain the target Attribute and
/// does not say what a `datasetId` that no instance of an existing Attribute has leads to; Andromeda treats the
/// missing instance as a missing target and answers `ResourceNotFound`
#[gtest]
#[tokio::test]
async fn o_att_del_0009_an_unknown_dataset_id_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let person_bob_id = namespace.urn("Person", "Bob")?;
    let nonexistent_dataset = namespace.dataset_id("Dataset", "nonexistent")?;
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
        .delete_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?)
        .instances(InstanceSelection::Dataset(&nonexistent_dataset))
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("speed", 2));
    Ok(())
}
