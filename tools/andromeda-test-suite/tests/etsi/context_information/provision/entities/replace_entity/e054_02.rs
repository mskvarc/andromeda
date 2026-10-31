//! A replace whose target id is faulty, missing or not held by the broker is refused (clauses 5.6.18 and 6.5.3.3).

use crate::{
    building_payloads::{BUILDING_REPLACE_TERMS, building_simple_attributes, pisa_tower_located_at_pisa},
    entity_operations::{context_link, create_entity_with_link, delete_entities},
};
use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::{content_type, request_to},
};
use googletest::prelude::*;
use http::{
    Method,
    StatusCode,
    header::{ALLOW, LINK},
};
use ngsi_ld::{
    http::UncheckedDocument,
    model::{APPLICATION_JSON, Lossless, RawJson},
};

/// Pins: Check that if the target entity ID is faulty an error is raised
/// Clause: 5.6.18, 6.5.3.3
#[gtest]
#[tokio::test]
async fn e054_02_01_replace_an_existing_entity_giving_an_invalid_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_REPLACE_TERMS).await?;
    let original = building_simple_attributes(&namespace, 1)?;
    create_entity_with_link(&broker, &vocabulary, &original).await?;
    let document = UncheckedDocument::from_entity(&pisa_tower_located_at_pisa(&namespace, 1)?, Lossless::Normalized)
        .set_member("id", &r#""invalidUri""#.parse::<RawJson>()?);
    let request = content_type(
        request_to(&broker, Method::PUT, "entities/invalidUri")?.document_as_is(&document),
        &APPLICATION_JSON,
    )?
    .append_header(LINK, context_link(&vocabulary)?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::BAD_REQUEST));
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}

/// Pins: Check that if the target entity ID is faulty an error is raised
/// Clause: 5.6.18, 6.5.3.3, 6.4.3, 6.3.2
/// Expected: an empty id sends the request to the collection `/entities/`, which serves `POST`, `GET` and `DELETE`
/// only (clause 6.4.3), so the wrong verb is answered 405 with `Allow` (clause 6.3.2), not 400; the operation
/// takes its target from `/entities/{entityId}` (clause 6.5.3.3)
#[gtest]
#[tokio::test]
async fn e054_02_02_replace_an_existing_entity_without_giving_an_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_REPLACE_TERMS).await?;
    let original = building_simple_attributes(&namespace, 1)?;
    create_entity_with_link(&broker, &vocabulary, &original).await?;
    let document = UncheckedDocument::from_entity(&pisa_tower_located_at_pisa(&namespace, "randomUUID")?, Lossless::Normalized);
    let request = content_type(request_to(&broker, Method::PUT, "entities/")?.document_as_is(&document), &APPLICATION_JSON)?
        .append_header(LINK, context_link(&vocabulary)?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        answered(StatusCode::METHOD_NOT_ALLOWED)
            .with_header_containing(ALLOW, "GET")
            .with_header_containing(ALLOW, "POST")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}

/// Pins: Check that if the target entity ID is faulty an error is raised
/// Clause: 5.6.18, 6.5.3.3
#[gtest]
#[tokio::test]
async fn e054_02_03_replace_an_existing_entity_giving_a_nonexistent_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_REPLACE_TERMS).await?;
    let original = building_simple_attributes(&namespace, 1)?;
    create_entity_with_link(&broker, &vocabulary, &original).await?;
    let client = broker.with_context(vocabulary.context())?;
    let replacement = pisa_tower_located_at_pisa(&namespace, "Nonexistent")?;
    let answer = client.with_responses().entities().replace(&replacement).send().await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
