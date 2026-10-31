//! A dataset id parameter is invalid request.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::DeleteEntity},
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: Delete Entity with a `datasetId` URL parameter is refused with 400 `InvalidRequest` and deletes nothing
/// Clause: 6.3.20, 6.5.3.2
/// Recorded: Delete Entity defines only the type URL parameter; clause 6.3.20 says an incompatible parameter
/// "should" be answered with `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_ent_del_2609_a_dataset_id_parameter_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(55).dataset_id(namespace.dataset_id("Property", "gps")?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // The operation defines no `datasetId` parameter; the request carries it as written.
    let request = broker.unchecked(&DeleteEntity::builder().id(&vehicle_id).build())?.uri(api_uri(
        &broker,
        &format!("entities/{vehicle_id}?datasetId=urn:ngsi-ld:Property:{namespace}-gps"),
    )?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    Ok(())
}
