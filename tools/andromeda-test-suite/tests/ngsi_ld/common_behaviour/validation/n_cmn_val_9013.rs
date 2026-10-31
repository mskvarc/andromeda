//! A misspelt sub attribute type is refused on append.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::AppendAttributes},
    model::{Entity, EntityFragment, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Append Entity Attributes with speed carrying an accuracy sub-Attribute typed Propertyy answers 400
/// `BadRequestData` naming /speed/accuracy and leaves speed as it was
/// Clause: 5.2.4, 4.5.2.2, 5.6.3.4, 5.5.4
///
/// Invalid type: Propertyy (the misspelt attribute type under test)
#[gtest]
#[tokio::test]
async fn n_cmn_val_9013_a_misspelt_sub_attribute_type_is_refused_on_append() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).property("speed", 1).build()?;
    broker.entities().create(&device).send().await?.done()?;
    let fragment = EntityFragment::builder().build()?;
    // The model refuses it (unknown attribute type `Propertyy` at speed.accuracy), so `speed` is written as text.
    let speed: RawJson = r#"{"type": "Property", "value": 2, "accuracy": {"type": "Propertyy", "value": 0.5}}"#.parse()?;
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("speed", &speed);
    let request = broker
        .unchecked(&AppendAttributes::builder().id(&device_id).fragment(&fragment).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("/speed/accuracy")
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(1).without_sub_attribute("accuracy")));
    Ok(())
}
