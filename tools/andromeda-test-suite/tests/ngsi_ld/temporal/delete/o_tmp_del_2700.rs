//! A dataset id parameter is invalid request.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::problem, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::DeleteTemporalEvolution},
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Delete Temporal Evolution of an Entity with a `datasetId` URL parameter is refused with 400
/// `InvalidRequest` and deletes nothing
/// Clause: 6.3.20, 6.19.3.2
/// Recorded: Delete Temporal Evolution of an Entity defines no `datasetId` URL parameter; clause 6.3.20 says an
/// incompatible parameter "should" be answered with `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_tmp_del_2700_a_dataset_id_parameter_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // The operation defines no `datasetId` parameter; the request carries it as written.
    let request = broker.unchecked(&DeleteTemporalEvolution { id: &vehicle_id })?.uri(api_uri(
        &broker,
        &format!("temporal/entities/{vehicle_id}?datasetId={}", namespace.dataset_id("Dataset", "fuel")?),
    )?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    Ok(())
}
