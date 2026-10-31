//! A replacement without a value member is bad request data.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    system_time::{self, SystemMember},
    written_json::json_request_to,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: Replace Attribute with the fragment {} and with a fragment holding only unitCode answers 400
/// `BadRequestData` each time, and the attribute keeps its value, unit and modifiedAt
/// Clause: 5.6.19.4, Table 5.2.5-1, 4.5.2, 5.5.4
#[gtest]
#[tokio::test]
async fn n_att_rpl_9001_a_replacement_without_a_value_member_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(50).unit_code(UnitCode::Kmh))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let entity_modified = system_time::of_entity(entity, SystemMember::ModifiedAt)?;
    let speed_modified = system_time::of_attribute(entity, &["speed"], SystemMember::ModifiedAt)?;
    for body in ["{}", r#"{"unitCode": "C62"}"#] {
        // A replacement without `type` or `value` is no attribute the model can write, so it is sent as written.
        let request = json_request_to(&broker, Method::PUT, &format!("entities/{vehicle_id}/attrs/speed"), body)?;
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
        let retrieved_again = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&vehicle_id)
            .system_attributes(SystemAttributes::Include)
            .send()
            .await?;
        let entity_again = retrieved_again.body();
        expect_that!(system_time::of_entity(entity_again, SystemMember::ModifiedAt)?, eq(entity_modified));
        expect_that!(entity_again, has_attribute("speed", property().value(50).unit_code(UnitCode::Kmh)));
        expect_that!(
            system_time::of_attribute(entity_again, &["speed"], SystemMember::ModifiedAt)?,
            eq(speed_modified)
        );
    }
    Ok(())
}
