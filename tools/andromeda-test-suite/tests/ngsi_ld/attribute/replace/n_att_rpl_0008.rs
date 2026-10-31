//! An attribute name that is not valid is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::ReplaceAttribute,
    model::{Attribute, AttributeName, Entity, ErrorType, Property, Relationship, Value},
};

/// Pins: Replace Attribute addressed to the Attribute name @invalid, which is not a valid name, is refused with 400
/// `BadRequestData`
/// Clause: 5.6.19.4, 4.6.2, 6.7.3.3
#[gtest]
#[tokio::test]
async fn n_att_rpl_0008_an_attribute_name_that_is_not_valid_is_bad_request_data() -> Result<()> {
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
    let attribute = Attribute::from(Property::builder(100).build()?);
    // The model refuses a name or an id of the path; the operation is built for a placeholder and sent to the path as written.
    let request = broker
        .unchecked(
            &ReplaceAttribute::builder()
                .id(&vehicle_id)
                .name(&"placeholder".parse::<AttributeName>()?)
                .attribute(&attribute)
                .build(),
        )?
        .uri(api_uri(&broker, &format!("entities/{vehicle_id}/attrs/@invalid"))?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
