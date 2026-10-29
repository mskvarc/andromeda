//! ETSI `006_03`: a batch delete whose body is not JSON, and one whose body is an empty array, are refused with 400
//! (clause 5.6.10).

use crate::suite_keywords::{ld_json_body_client, post_text, refused, with_doubled_separator};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::BatchDelete},
    model::{APPLICATION_LD_JSON, Entity, ErrorType, Lossless},
};

/// Pins: Check that one cannot delete a batch of entities with an invalid request
/// Clause: 5.6.10
#[gtest]
#[tokio::test]
async fn e006_03_01_invalid_json() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let ids = [namespace.urn("Building", "randomUUID")?];
    let entity = Entity::builder().id(namespace.urn("Building", "randomUUID")?).entity_type("Building").build()?;
    let document = UncheckedDocument::from_entity(&entity, Lossless::Normalized);
    let request = client.unchecked(&BatchDelete { ids: &ids })?.documents(&[document]);
    let answer = client.send_unchecked(with_doubled_separator(request)).await;
    verify_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title())?;
    Ok(())
}

/// Pins: Check that one cannot delete a batch of entities with an invalid request
/// Clause: 5.6.10
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e006_03_02_empty_json() -> Result<()> {
    let broker = broker::front()?;
    let request = post_text(&broker, "entityOperations/delete", &APPLICATION_LD_JSON, "[]")?;
    refused(&broker, request, ErrorType::BadRequestData).await
}
