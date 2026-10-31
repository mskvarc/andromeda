//! A json property array reads back as the same array.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Input, Lossless, NgsiLdMediaType, Output},
};

/// The entity as the request writes it.
const BODY: &str = r#"{
    "id": "urn:ngsi-ld:Building:{{ns}}-1",
    "type": "Building_{{ns}}",
    "jsonProperty": {
        "type": "JsonProperty",
        "json": [
            {
                "item": 1
            },
            {
                "item": 2
            }
        ]
    }
}"#;

/// Pins: an entity created with a `JsonProperty` whose json is an array of two objects reads back with a
/// `JsonProperty` whose json is that two-item array
/// Clause: 5.6.1, 4.5.24, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_0012_a_json_property_array_reads_back_as_the_same_array() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let body = BODY.replace("{{ns}}", namespace.token());
    let building = Entity::<Input>::parse(&body, Lossless::Normalized)?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let expected = Entity::<Output>::parse(&body, Lossless::Normalized)?;
    expect_that!(retrieved.body(), eq(&expected));
    Ok(())
}
