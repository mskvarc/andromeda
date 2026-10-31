//! Concise typeless instances in an array are instances.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Input, Lossless, NgsiLdMediaType, Output},
};

/// The entity as the request writes it.
const BODY: &str = r#"{
    "id": "urn:ngsi-ld:Device:{{ns}}-1",
    "type": "Device_{{ns}}",
    "speed": [
        {
            "value": 25,
            "datasetId": "urn:ngsi-ld:Dataset:{{ns}}-a"
        },
        {
            "value": 30,
            "datasetId": "urn:ngsi-ld:Dataset:{{ns}}-b"
        }
    ]
}"#;

/// The entity as the broker reads it back: two Property instances, each with exactly a type, a value and a datasetId.
const READ_BACK: &str = r#"{
    "id": "urn:ngsi-ld:Device:{{ns}}-1",
    "type": "Device_{{ns}}",
    "speed": [
        {
            "type": "Property",
            "value": 25,
            "datasetId": "urn:ngsi-ld:Dataset:{{ns}}-a"
        },
        {
            "type": "Property",
            "value": 30,
            "datasetId": "urn:ngsi-ld:Dataset:{{ns}}-b"
        }
    ]
}"#;

/// Pins: Create Entity with an attribute written as an array of two typeless objects each holding a value and a
/// `datasetId` stores two Property instances, each read back with exactly its type, value and `datasetId`
/// Clause: 4.5.2.3, 4.5.5, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9001_concise_typeless_instances_in_an_array_are_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device = Entity::<Input>::parse(&BODY.replace("{{ns}}", namespace.token()), Lossless::Concise)?;
    broker.entities().create(&device).form(Lossless::Concise).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let expected = Entity::<Output>::parse(&READ_BACK.replace("{{ns}}", namespace.token()), Lossless::Normalized)?;
    expect_that!(retrieved.body(), eq(&expected));
    Ok(())
}
