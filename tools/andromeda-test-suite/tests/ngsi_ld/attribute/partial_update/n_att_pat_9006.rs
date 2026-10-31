//! A dataset id parameter on an attribute write is an invalid request.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    wire::{api_uri, request_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{PartialAttributeUpdate, ReplaceAttribute},
    },
    model::{Attribute, AttributeKind, AttributeName, Change, Entity, ErrorType, InstanceDeletion, NgsiLdMediaType, Property},
};

/// Pins: Partial Attribute Update and Replace Attribute with a `datasetId` URL parameter, naming an existing
/// instance, an unknown one, or with an NGSI-LD Null fragment, answer 400 `InvalidRequest` with a title, and both
/// instances keep their values
/// Clause: 6.3.20, Table 6.7.3.1-1, Table 6.7.3.3-1, 5.6.4.4, 5.6.19.4
#[gtest]
#[tokio::test]
async fn n_att_pat_9006_a_dataset_id_parameter_on_an_attribute_write_is_an_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_dataset_parameter_id = namespace.urn("Vehicle", "dataset-parameter")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let gps_dataset = namespace.dataset_id("Property", "gps")?;
    let speedometer_dataset = namespace.dataset_id("Property", "speedometer")?;
    let vehicle_dataset_parameter = Entity::builder()
        .id(vehicle_dataset_parameter_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(60).dataset_id(namespace.dataset_id("Property", "gps")?))
        .property("speed", Property::builder(61).dataset_id(namespace.dataset_id("Property", "speedometer")?))
        .build()?;
    broker.entities().create(&vehicle_dataset_parameter).send().await?.done()?;
    // The model refuses this attribute, so the body is sent as written.
    let request = request_to(
        &broker,
        Method::PATCH,
        &format!("entities/{vehicle_dataset_parameter_id}/attrs/speed?datasetId=urn%3Angsi-ld%3AProperty%3A{namespace}-speedometer"),
    )?
    .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
    .body(Bytes::from("{\n    \"value\": 65\n}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The model refuses this attribute, so the body is sent as written.
    let request_2 = request_to(
        &broker,
        Method::PATCH,
        &format!("entities/{vehicle_dataset_parameter_id}/attrs/speed?datasetId=urn%3Angsi-ld%3AProperty%3A{namespace}-unknown"),
    )?
    .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
    .body(Bytes::from("{\n    \"value\": 65\n}"));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    let deletion = Change::Delete(InstanceDeletion::new(AttributeKind::Property));
    // The operation defines no `datasetId` parameter; the request carries it as written.
    let request_3 = broker
        .unchecked(
            &PartialAttributeUpdate::builder()
                .id(&vehicle_dataset_parameter_id)
                .name(&"speed".parse::<AttributeName>()?)
                .attribute(&deletion)
                .build(),
        )?
        .uri(api_uri(
            &broker,
            &format!("entities/{vehicle_dataset_parameter_id}/attrs/speed?datasetId=urn%3Angsi-ld%3AProperty%3A{namespace}-gps"),
        )?);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    let attribute = Attribute::from(Property::builder(100).build()?);
    // The operation defines no `datasetId` parameter; the request carries it as written.
    let request_4 = broker
        .unchecked(
            &ReplaceAttribute::builder()
                .id(&vehicle_dataset_parameter_id)
                .name(&"speed".parse::<AttributeName>()?)
                .attribute(&attribute)
                .build(),
        )?
        .uri(api_uri(
            &broker,
            &format!("entities/{vehicle_dataset_parameter_id}/attrs/speed?datasetId=urn%3Angsi-ld%3AProperty%3A{namespace}-speedometer"),
        )?);
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_dataset_parameter_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("speed", 2));
    expect_that!(entity, has_attribute("speed", any_attribute().dataset_id(gps_dataset).value(60)));
    expect_that!(entity, has_attribute("speed", any_attribute().dataset_id(speedometer_dataset).value(61)));
    Ok(())
}
