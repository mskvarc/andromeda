//! An invalid attribute name is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::problem, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::DeleteTemporalAttribute,
    model::{AttributeName, ErrorType, Property, evolution::TemporalEntity},
};

/// Pins: Delete Attribute addressed to an Attribute name that is not a valid name is refused with 400
/// `BadRequestData`, with a title
/// Clause: 5.6.13.4, 6.21.3.1, 4.6.2
#[gtest]
#[tokio::test]
async fn n_tmp_adl_0006_an_invalid_attribute_name_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(67).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-09-01T13:05:00Z".parse()?))
        .property(
            "fuelLevel",
            Property::builder(40)
                .dataset_id(namespace.dataset_id("Dataset", "fuel")?)
                .observed_at("2020-09-01T14:07:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // The model refuses a name or an id of the path; the operation is built for a placeholder and sent to the path as written.
    let request = broker
        .unchecked(
            &DeleteTemporalAttribute::builder()
                .id(&vehicle_id)
                .name(&"placeholder".parse::<AttributeName>()?)
                .build(),
        )?
        .uri(api_uri(&broker, &format!("temporal/entities/{vehicle_id}/attrs/invalid(Name"))?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
