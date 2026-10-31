//! A sub attribute of another dataset is added by a partial update.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property, Value},
};

/// Pins: Partial Attribute Update of speed with value 20 and an accuracy of dataset b onto accuracy of dataset a
/// answers 204 and leaves speed 20 with both accuracy instances
/// Clause: 4.5.5.1, 5.5.8, 5.6.4.4
#[gtest]
#[tokio::test]
async fn n_att_pat_9023_a_sub_attribute_of_another_dataset_is_added_by_a_partial_update() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_partial_id = namespace.urn("Vehicle", "partial")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_partial = Entity::builder()
        .id(vehicle_partial_id.clone())
        .entity_type(vehicle_type)
        .property(
            "speed",
            Property::builder(10).property(
                "accuracy",
                Property::builder(Value::Number("0.1".parse()?)).dataset_id(namespace.dataset_id("Dataset", "a")?),
            ),
        )
        .build()?;
    broker.entities().create(&vehicle_partial).send().await?.done()?;
    // The model drops the `type` of a sub-attribute the patch carries beside a bare value, so the patch is sent as written.
    let request = json_request_to(
        &broker,
        Method::PATCH,
        &format!("entities/{vehicle_partial_id}/attrs/speed"),
        &format!(r#"{{"value": 20, "accuracy": {{"type": "Property", "value": 0.2, "datasetId": "urn:ngsi-ld:Dataset:{namespace}-b"}}}}"#),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_partial_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_attribute(
            "speed",
            property()
                .value(20)
                .sub_attribute_instances("accuracy", 2)
                .sub_attribute(
                    "accuracy",
                    property()
                        .dataset_id(namespace.dataset_id("Dataset", "a")?)
                        .value(Value::Number("0.1".parse()?))
                )
                .sub_attribute(
                    "accuracy",
                    property()
                        .dataset_id(namespace.dataset_id("Dataset", "b")?)
                        .value(Value::Number("0.2".parse()?))
                )
        )
    );
    Ok(())
}
