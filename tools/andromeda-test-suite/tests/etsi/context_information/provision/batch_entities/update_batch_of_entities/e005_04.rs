//! ETSI `005_04`: a batch update whose body is not JSON is refused with 400, and one whose entity carries a null Property
//! is answered 207 with the entity in error (clause 5.6.9).

use crate::suite_keywords::{expect_bad_request_entity, ld_json_body_client, partial_batch_answer, with_doubled_separator};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::BatchUpdate},
    model::{Entity, ErrorType, Lossless, RawJson},
};

/// Pins: Check that one cannot update a batch of entities with an invalid request
/// Clause: 5.6.9
#[gtest]
#[tokio::test]
async fn e005_04_01_invalid_json() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let entity = Entity::builder().id(namespace.urn("Building", "randomUUID")?).entity_type("Building").build()?;
    let document = UncheckedDocument::from_entity(&entity, Lossless::Normalized);
    let request = client
        .unchecked(&BatchUpdate::builder().fragments(&[entity.into_fragment()]).build())?
        .documents(&[document]);
    let answer = client.send_unchecked(with_doubled_separator(request)).await;
    verify_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title())?;
    Ok(())
}

/// Pins: Check that one cannot update a batch of entities with an invalid request
/// Clause: 5.6.9
#[gtest]
#[tokio::test]
async fn e005_04_02_invalid_json_ld() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name"]).await?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let entity = Entity::builder().id(namespace.urn("Building", "randomUUID")?).entity_type("Building").build()?;
    let null_name: RawJson = r#"{"type":"Property","value":null}"#.parse()?;
    let document = UncheckedDocument::from_entity(&entity, Lossless::Normalized).set_member("name", &null_name);
    let request = client
        .unchecked(&BatchUpdate::builder().fragments(&[entity.into_fragment()]).build())?
        .documents(&[document]);
    let result = partial_batch_answer(&client, request).await?;
    expect_bad_request_entity(&result)?;
    Ok(())
}
