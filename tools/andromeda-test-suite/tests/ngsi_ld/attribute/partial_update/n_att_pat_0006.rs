//! An empty ld json fragment without a context is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::model::{Entity, ErrorType, Property, Relationship, Value};

/// Pins: Partial Attribute Update with the empty Fragment {} sent as application/ld+json, which carries no
/// @context, is refused with 400 `BadRequestData`
/// Clause: 6.3.5, 5.6.4.4
#[gtest]
#[tokio::test]
async fn n_att_pat_0006_an_empty_ld_json_fragment_without_a_context_is_bad_request_data() -> Result<()> {
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
    // The body goes as application/ld+json, which no typed operation sends, so it is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_id}/attrs/speed"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from("{}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
