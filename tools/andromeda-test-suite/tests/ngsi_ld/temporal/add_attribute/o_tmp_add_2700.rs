//! A dataset id parameter is invalid request.

use andromeda_test_suite::{broker, evolution_matcher::has_evolution, namespace::Namespace, outcome::Upserted, problem::problem, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::AddTemporalAttributes},
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Add Attributes to Temporal Evolution with a `datasetId` URL parameter is refused with 400 `InvalidRequest`
/// and adds no instance
/// Clause: 6.3.20, 6.20.3.1
/// Recorded: Add Attributes to Temporal Evolution defines no `datasetId` URL parameter; clause 6.3.20 says an
/// incompatible parameter "should" be answered with `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_tmp_add_2700_a_dataset_id_parameter_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let fuel_dataset = namespace.dataset_id("Dataset", "fuel")?;
    let vehicle = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle).send().await?.created()?;
    let instances = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .build()?
        .attributes;
    // The operation defines no datasetId parameter, so the request is sent to the path as written.
    let request = broker
        .unchecked(&AddTemporalAttributes::builder().id(&vehicle_id).attributes(&instances).build())?
        .uri(api_uri(&broker, &format!("temporal/entities/{vehicle_id}/attrs?datasetId={fuel_dataset}"))?);
    let refused = broker.send_unchecked(request).await;
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let evolution = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(evolution.body(), has_evolution("speed", len(eq(1))));
    Ok(())
}
