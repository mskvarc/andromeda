//! A dataset id parameter is invalid request.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    wire::api_uri,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::ReplaceAttribute},
    model::{Attribute, AttributeName, Entity, ErrorType, NgsiLdMediaType, Property, Value},
};

/// Pins: Replace Attribute with a `datasetId` URL parameter naming an instance the Attribute has is refused with
/// 400 `InvalidRequest` and leaves both instances unchanged
/// Clause: 6.3.20, 6.7.3.3, 5.6.19.4
/// Recorded: Table 6.7.3.3-1 defines type as the only URL parameter of Replace Attribute, which reads the
/// `datasetId` from the Fragment (clause 5.6.19.4); clause 6.3.20 says a parameter incompatible with the operation
/// "should" be answered with `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_att_rpl_0011_a_dataset_id_parameter_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let speedometer_dataset = namespace.dataset_id("Property", "speedometer")?;
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
    let attribute = Attribute::from(Property::builder(100).build()?);
    // The operation defines no `datasetId` parameter; the request carries it as written.
    let request = broker
        .unchecked(
            &ReplaceAttribute::builder()
                .id(&vehicle_id)
                .name(&"speed".parse::<AttributeName>()?)
                .attribute(&attribute)
                .build(),
        )?
        .uri(api_uri(
            &broker,
            &format!("entities/{vehicle_id}/attrs/speed?datasetId=urn:ngsi-ld:Property:{namespace}-speedometer"),
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
    expect_that!(entity, has_instances("speed", 2));
    expect_that!(
        entity,
        has_attribute("speed", any_attribute().dataset_id(speedometer_dataset).value(Value::Number("54.5".parse()?)))
    );
    expect_that!(entity, has_attribute("speed", any_attribute().dataset_id(gps_dataset).value(55)));
    Ok(())
}
