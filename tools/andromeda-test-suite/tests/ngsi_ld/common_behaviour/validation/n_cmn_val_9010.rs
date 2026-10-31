//! A supplied previous value is ignored not stored.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, Lossless, NgsiLdMediaType, RawJson, Value},
};

/// Pins: Create Entity with a Property carrying `previousValue` answers 201, and the entity reads back with the
/// value and without `previousValue`
/// Clause: 5.2.5, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_cmn_val_9010_a_supplied_previous_value_is_ignored_not_stored() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type).build()?;
    // The model refuses it (member `previousValue` is not allowed here at temperature), so `temperature` is written as text.
    let temperature: RawJson = r#"{"type": "Property", "value": 21.5, "previousValue": 9.9}"#.parse()?;
    let document = UncheckedDocument::from_entity(&building, Lossless::Normalized).set_member("temperature", &temperature);
    let request = broker.unchecked(&CreateEntity::builder().entity(&building).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "temperature",
            any_attribute().value(Value::Number("21.5".parse()?)).without_sub_attribute("previousValue")
        )
    );
    Ok(())
}
