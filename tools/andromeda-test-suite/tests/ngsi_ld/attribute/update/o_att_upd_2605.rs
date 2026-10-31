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
    http::{headers::Accept, operation::UpdateAttributes},
    model::{Entity, EntityFragment, ErrorType, NgsiLdMediaType, Property, Value},
};

/// Pins: Update Entity Attributes with a `datasetId` URL parameter is refused with 400 `InvalidRequest` and changes
/// nothing
/// Clause: 6.3.20, 6.6.3
/// Recorded: Update Entity Attributes defines no `datasetId` URL parameter; clause 6.3.20 says an incompatible
/// parameter "should" be answered with `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_att_upd_2605_a_dataset_id_parameter_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let gps_dataset = namespace.dataset_id("Property", "gps")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(55).dataset_id(namespace.dataset_id("Property", "gps")?))
        .property(
            "speed",
            Property::builder(Value::Number("54.5".parse()?)).dataset_id(namespace.dataset_id("Property", "speedometer")?),
        )
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("speed", Property::builder(99).dataset_id(namespace.dataset_id("Property", "gps")?))
        .build()?;
    // The operation defines no `datasetId` parameter; the request carries it as written.
    let request = broker
        .unchecked(&UpdateAttributes::builder().id(&vehicle_id).fragment(&fragment).build())?
        .uri(api_uri(
            &broker,
            &format!("entities/{vehicle_id}/attrs?datasetId=urn:ngsi-ld:Property:{namespace}-gps"),
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
    expect_that!(entity, has_attribute("speed", any_attribute().dataset_id(gps_dataset).value(55)));
    Ok(())
}
