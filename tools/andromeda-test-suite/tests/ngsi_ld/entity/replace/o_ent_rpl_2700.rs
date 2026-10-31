//! A dataset id parameter is invalid request.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: Replace Entity with a `datasetId` URL parameter is refused with 400 `InvalidRequest` and replaces nothing
/// Clause: 6.3.20, 6.5.3.4
/// Recorded: Replace Entity defines no `datasetId` URL parameter; clause 6.3.20 says an incompatible parameter
/// "should" be answered with `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_ent_rpl_2700_a_dataset_id_parameter_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let gps_dataset = namespace.dataset_id("Property", "gps")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(55).dataset_id(namespace.dataset_id("Property", "gps")?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // The operation defines no `datasetId` parameter, and the replacement leaves out the `id` the model requires, so
    // the request is sent as written.
    let request = json_request_to(
        &broker,
        Method::PUT,
        &format!("entities/{vehicle_id}?datasetId=urn:ngsi-ld:Property:{namespace}-gps"),
        &format!(
            "{{\n    \"type\": \"Vehicle_{namespace}\",\n    \"speed\": {{ \"type\": \"Property\", \"value\": 99, \"datasetId\": \"urn:ngsi-ld:Property:{namespace}-gps\" }}\n}}"
        ),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", property().value(55).with_dataset_id(gps_dataset)));
    Ok(())
}
