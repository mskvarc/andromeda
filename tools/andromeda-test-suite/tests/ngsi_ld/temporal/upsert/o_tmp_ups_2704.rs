//! A dataset id parameter is invalid request.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Create or Update Temporal Evolution with a `datasetId` URL parameter is refused with 400 `InvalidRequest` and
/// creates nothing
/// Clause: 6.3.20, 6.18.3.1
/// Recorded: Create Temporal Evolution defines no `datasetId` URL parameter; clause 6.3.20 says an incompatible parameter
/// "should" be answered with `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_tmp_ups_2704_a_dataset_id_parameter_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let fuel = namespace.dataset_id("Dataset", "fuel")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .build()?;
    // The operation defines no `datasetId` parameter; the request carries it as written.
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&vehicle_evolution).build())?
        .uri(api_uri(&broker, &format!("temporal/entities?datasetId={fuel}"))?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let missing = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&missing, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
