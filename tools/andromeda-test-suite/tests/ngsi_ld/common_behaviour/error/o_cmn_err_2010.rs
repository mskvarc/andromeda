//! Temporal write operations with an unknown parameter are invalid request.

use andromeda_test_suite::{broker, evolution_matcher::has_evolution, namespace::Namespace, outcome::Upserted, problem::problem, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{AddTemporalAttributes, DeleteTemporalAttribute, DeleteTemporalEvolution},
    },
    model::{AttributeName, ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Add Attributes to Temporal Evolution, Delete Temporal Attribute, and Delete Temporal Evolution with a URL
/// parameter they do not define answer `InvalidRequest` and change nothing
/// Clause: 6.3.20, 5.6.12, 5.6.13, 5.6.16, 5.6.11
/// Recorded: 6.3.20 says an incompatible parameter "should" give `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_cmn_err_2010_temporal_write_operations_with_an_unknown_parameter_are_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device_evolution = TemporalEntity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("speed", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&device_evolution).send().await?.created()?;
    let instances = TemporalEntity::builder()
        .id(device_id.clone())
        .entity_type(device_type)
        .property("speed", Property::builder(2).observed_at("2020-01-02T00:00:00Z".parse()?))
        .build()?
        .attributes;
    // The operation defines no `invalidParam` parameter; the request carries it as written.
    let request = broker
        .unchecked(&AddTemporalAttributes::builder().id(&device_id).attributes(&instances).build())?
        .uri(api_uri(&broker, &format!("temporal/entities/{device_id}/attrs?invalidParam=invalidValue"))?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    // The operation defines no `invalidParam` parameter; the request carries it as written.
    let request_2 = broker
        .unchecked(
            &DeleteTemporalAttribute::builder()
                .id(&device_id)
                .name(&"speed".parse::<AttributeName>()?)
                .build(),
        )?
        .uri(api_uri(
            &broker,
            &format!("temporal/entities/{device_id}/attrs/speed?invalidParam=invalidValue"),
        )?);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    // The operation defines no `invalidParam` parameter; the request carries it as written.
    let request_3 = broker
        .unchecked(&DeleteTemporalEvolution { id: &device_id })?
        .uri(api_uri(&broker, &format!("temporal/entities/{device_id}?invalidParam=invalidValue"))?);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&device_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", len(eq(1))));
    Ok(())
}
