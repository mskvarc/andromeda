//! An expires at written with an offset is bad request data at every depth.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity with an `expiresAt` written with a UTC offset, on an Attribute or on its sub-attribute, answers
/// 400 `BadRequestData` and stores nothing
/// Clause: 4.6.3, 4.22, 5.6.1.4
/// Recorded: clause 4.6.3 has a `DateTime` end in "Z", always expressed in UTC, so an offset is not a `DateTime`
#[gtest]
#[tokio::test]
async fn o_ent_ret_0033_an_expires_at_written_with_an_offset_is_bad_request_data_at_every_depth() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type).build()?;
    for temperature in [
        r#"{"type": "Property", "value": 21, "expiresAt": "2099-01-01T02:00:00+02:00"}"#,
        r#"{"type": "Property", "value": 21, "accuracy": {"type": "Property", "value": 0.5, "expiresAt": "2099-01-01T02:00:00+02:00"}}"#,
    ] {
        // The model refuses an offset in `expiresAt`, so `temperature` is written as text.
        let temperature: RawJson = temperature.parse()?;
        let document = UncheckedDocument::from_entity(&building, Lossless::Normalized).set_member("temperature", &temperature);
        let request = broker.unchecked(&CreateEntity::builder().entity(&building).build())?.document(&document);
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
        let stored = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&building_id)
            .send()
            .await;
        expect_that!(&stored, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    }
    Ok(())
}
