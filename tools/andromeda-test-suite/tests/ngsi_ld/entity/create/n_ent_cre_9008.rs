//! A concise empty array is a property value.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{attribute_members, has_attribute},
    namespace::Namespace,
    outcome::Done,
    simplified_matcher::{has_length_at, has_value},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Entity, Input, Lossless, NgsiLdMediaType, Value},
};

/// The entity as the request writes it.
const BODY: &str = r#"{
    "id": "urn:ngsi-ld:EmptyProbe:{{ns}}-1",
    "type": "EmptyProbe_{{ns}}",
    "tags": []
}"#;

/// Pins: Create Entity with a concise member whose value is the empty array answers 201, and the member reads back
/// as a Property holding the empty array, and as the empty array in the concise and simplified representations
/// Clause: 5.6.1, 4.5.2.3, 4.5.3, 4.5.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9008_a_concise_empty_array_is_a_property_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let probe_id = namespace.urn("EmptyProbe", 1)?;
    let probe = Entity::<Input>::parse(&BODY.replace("{{ns}}", namespace.token()), Lossless::Concise)?;
    broker.entities().create(&probe).form(Lossless::Concise).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&probe_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("tags", property().value(Value::Array(Vec::new()))));
    expect_that!(attribute_members(entity, "tags"), some(eq(2)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&probe_id)
        .form(Lossless::Concise)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    expect_that!(retrieved_2.body(), has_attribute("tags", property().value(Value::Array(Vec::new()))));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&probe_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    expect_that!(retrieved_3.body(), has_value("tags", Value::Array(Vec::new())));
    expect_that!(retrieved_3.body(), has_length_at("tags", [], 0));
    Ok(())
}
