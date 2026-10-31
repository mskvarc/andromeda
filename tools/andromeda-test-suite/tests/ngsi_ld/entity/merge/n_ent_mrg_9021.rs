//! A null sub attribute deletes only the instance of its dataset.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property, Value},
};

/// Pins: Merge Entity of speed with the null accuracy of dataset a onto a default accuracy and one of dataset a
/// leaves the default accuracy alone
/// Clause: 4.5.5.1, 5.5.12, 5.6.17.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9021_a_null_sub_attribute_deletes_only_the_instance_of_its_dataset() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_deletion_id = namespace.urn("Vehicle", "deletion")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_deletion = Entity::builder()
        .id(vehicle_deletion_id.clone())
        .entity_type(vehicle_type)
        .property(
            "speed",
            Property::builder(10).property("accuracy", Value::Number("0.1".parse()?)).property(
                "accuracy",
                Property::builder(Value::Number("0.5".parse()?)).dataset_id(namespace.dataset_id("Dataset", "a")?),
            ),
        )
        .build()?;
    broker.entities().create(&vehicle_deletion).send().await?.done()?;
    // The Property builder refuses the NGSI-LD Null as a value, so the fragment is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_deletion_id}"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\"speed\": {{\"type\": \"Property\", \"value\": 20, \"accuracy\": {{\"type\": \"Property\", \"value\": \"urn:ngsi-ld:null\", \"datasetId\": \"urn:ngsi-ld:Dataset:{namespace}-a\"}}}}}}"
        )));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_deletion_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "speed",
            any_attribute()
                .sub_attribute_instances("accuracy", 1)
                .sub_attribute("accuracy", property().value(Value::Number("0.1".parse()?)).without(Member::DatasetId))
        )
    );
    Ok(())
}
