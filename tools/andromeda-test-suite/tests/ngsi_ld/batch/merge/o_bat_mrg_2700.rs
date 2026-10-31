//! A dataset id parameter is invalid request.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    wire::api_uri,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::BatchMerge},
    model::{Entity, EntityFragment, ErrorType, NgsiLdMediaType},
};

/// Pins: Batch Entity Merge with a `datasetId` URL parameter is refused with 400 `InvalidRequest` and merges
/// nothing
/// Clause: 6.3.20, 5.6.20
/// Recorded: Batch Entity Merge defines no `datasetId` URL parameter; clause 6.3.20 says an incompatible parameter
/// "should" be answered with `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_bat_mrg_2700_a_dataset_id_parameter_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 55)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(vehicle_id.clone(), vehicle_type.clone())
        .property("speed", 99)
        .build()?];
    // The operation defines no `datasetId` parameter; the request carries it as written.
    let request = broker.unchecked(&BatchMerge::builder().fragments(&fragments).build())?.uri(api_uri(
        &broker,
        &format!("entityOperations/merge?datasetId=urn:ngsi-ld:Property:{namespace}-gps"),
    )?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(55)));
    Ok(())
}
