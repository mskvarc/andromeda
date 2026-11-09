//! A delete on the attributes collection is 405.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    broker,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    wire::{header_values, request_to},
};
use googletest::prelude::*;
use http::{Method, StatusCode, header::ALLOW};
use ngsi_ld::model::{Entity, Property, Relationship, Value};

/// Pins: DELETE on /entities/{entityId}/attrs, which names no Attribute and serves only POST and PATCH, is refused
/// with 405 and an Allow header naming the supported methods
/// Clause: 5.6.5.4, 6.6.3, 6.3.2
/// Recorded: clause 5.6.5.4 asks for `BadRequestData` when the target Attribute name is not present, but the HTTP
/// binding has no Delete Attribute resource without an attrId: /entities/{entityId}/attrs (clause 6.6.3) serves only
/// POST and PATCH, so Andromeda answers the request as a method that resource does not serve (IETF RFC 9110, clause
/// 15.5.6)
#[gtest]
#[tokio::test]
async fn o_att_del_0006_a_delete_on_the_attributes_collection_is_405() -> Result<()> {
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
    // No operation of the specification deletes the attributes collection, so the request is sent as written.
    let request = request_to(&broker, Method::DELETE, &format!("entities/{vehicle_id}/attrs"))?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        answered(StatusCode::METHOD_NOT_ALLOWED)
            .with_header_containing(ALLOW, "POST")
            .with_header_containing(ALLOW, "PATCH")
    );
    let allowed = answer.answered_headers().map_or_default(|headers| header_values(headers, &ALLOW));
    expect_that!(allowed.iter().any(|line| line.contains("DELETE")), eq(false));
    Ok(())
}
