//! ETSI `004_06`: a batch upsert whose body is not JSON is refused with 400, and one whose entity holds a null Property is
//! answered 207 with that entity in error (clause 5.6.8).

use crate::{
    suite_keywords::{expect_bad_request_entity, ld_json_body_client, partial_batch_answer, refused, with_doubled_separator},
    suite_payloads::bare_building,
};
use andromeda_test_suite::{broker, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::{UncheckedDocument, operation::BatchUpsert},
    model::{ErrorType, Lossless, RawJson},
};

/// Pins: Check that one cannot upsert a batch of entities with an invalid request
/// Clause: 5.6.8
#[gtest]
#[tokio::test]
async fn e004_06_01_invalid_json() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let entity = bare_building(&namespace, "randomUUID")?;
    let document = UncheckedDocument::from_entity(&entity, Lossless::Normalized);
    let request = client.unchecked(&BatchUpsert::builder().entities(&[entity]).build())?.documents(&[document]);
    refused(&client, with_doubled_separator(request), ErrorType::InvalidRequest).await
}

/// Pins: Check that one cannot upsert a batch of entities with an invalid request
/// Clause: 5.6.8
#[gtest]
#[tokio::test]
async fn e004_06_02_invalid_json_ld() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name"]).await?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let entity = bare_building(&namespace, "randomUUID")?;
    let null_name: RawJson = r#"{"type":"Property","value":null}"#.parse()?;
    let document = UncheckedDocument::from_entity(&entity, Lossless::Normalized).set_member("name", &null_name);
    let request = client.unchecked(&BatchUpsert::builder().entities(&[entity]).build())?.documents(&[document]);
    let result = partial_batch_answer(&client, request).await?;
    expect_bad_request_entity(&result)
}
