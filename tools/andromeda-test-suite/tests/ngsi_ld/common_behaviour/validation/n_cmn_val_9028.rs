//! A relationship object with a space is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity whose Relationship `locatedAt` has the object urn:ngsi-ld:City:<ns>-has space answers 400
/// `BadRequestData` and stores no Entity
/// Clause: 4.5.3.2, 5.2.4, 5.6.1.4; IETF RFC 3986 section 2 and appendix A
#[gtest]
#[tokio::test]
async fn n_cmn_val_9028_a_relationship_object_with_a_space_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_spaced_object_id = namespace.urn("Building", "spaced-object")?;
    let building_type = namespace.entity_type("Building")?;
    let building_spaced_object = Entity::builder().id(building_spaced_object_id.clone()).entity_type(building_type).build()?;
    // The model refuses it (invalid identifier at locatedAt), so `locatedAt` is written as text.
    let located_at: RawJson = format!(r#"{{"type": "Relationship", "object": "urn:ngsi-ld:City:{namespace}-has space"}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&building_spaced_object, Lossless::Normalized).set_member("locatedAt", &located_at);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_spaced_object).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_spaced_object_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
