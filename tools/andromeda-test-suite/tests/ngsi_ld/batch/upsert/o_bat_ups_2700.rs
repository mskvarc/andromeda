//! A dataset id parameter is invalid request.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
    wire::api_uri,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::BatchUpsert},
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Batch Entity Upsert with a `datasetId` URL parameter is refused with 400 `InvalidRequest` and creates
/// nothing
/// Clause: 6.3.20, 6.15.3.1
/// Recorded: Batch Entity Upsert defines no `datasetId` URL parameter; clause 6.3.20 says an incompatible parameter
/// "should" be answered with `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_bat_ups_2700_a_dataset_id_parameter_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let entities = [Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 55)
        .build()?];
    // The operation defines no `datasetId` parameter; the request carries it as written.
    let request = broker.unchecked(&BatchUpsert::builder().entities(&entities).build())?.uri(api_uri(
        &broker,
        &format!("entityOperations/upsert?datasetId=urn:ngsi-ld:Property:{namespace}-gps"),
    )?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
