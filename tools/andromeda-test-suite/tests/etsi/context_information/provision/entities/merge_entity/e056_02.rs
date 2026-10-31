//! A merge whose target id is faulty, missing or not held by the broker is refused (clauses 5.6.17 and 6.5.3.4).

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
/// Clause: 5.6.17, 6.5.3.4
#[gtest]
#[tokio::test]
async fn e056_02_01_merge_an_existing_entity_giving_an_invalid_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_REPLACE_TERMS).await?;
    let original = building_simple_attributes(&namespace, 1)?;
    create_entity_with_link(&broker, &vocabulary, &original).await?;
    let document = UncheckedDocument::from_entity(&pisa_tower_located_at_pisa(&namespace, 1)?, Lossless::Normalized)
        .set_member("id", &r#""invalidUri""#.parse::<RawJson>()?);
    let request = content_type(
        request_to(&broker, Method::PATCH, "entities/invalidUri")?.document_as_is(&document),
        &APPLICATION_JSON,
    )?
    .append_header(LINK, context_link(&vocabulary)?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::BAD_REQUEST));
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}

/// Pins: Check that if the target entity ID is faulty an error is raised
/// Clause: 5.6.17, 6.5.3.4, 6.4.3, 6.3.2
/// Expected: an empty id sends the request to the collection `/entities/`, which serves `POST`, `GET` and `DELETE`
/// only (clause 6.4.3), so the wrong verb is answered 405 with `Allow` (clause 6.3.2), not 400; the operation
/// takes its target from `/entities/{entityId}` (clause 6.5.3.4)
#[gtest]
#[tokio::test]
async fn e056_02_02_merge_an_existing_entity_without_giving_an_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_REPLACE_TERMS).await?;
    let original = building_simple_attributes(&namespace, 1)?;
    create_entity_with_link(&broker, &vocabulary, &original).await?;
    let document =
        UncheckedDocument::from_entity(&pisa_tower_located_at_pisa(&namespace, 1)?, Lossless::Normalized).set_member("id", &r#""""#.parse::<RawJson>()?);
    let request = content_type(request_to(&broker, Method::PATCH, "entities/")?.document_as_is(&document), &APPLICATION_JSON)?
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
/// Clause: 5.6.17, 6.5.3.4
#[gtest]
#[tokio::test]
async fn e056_02_03_merge_an_existing_entity_giving_a_nonexistent_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_REPLACE_TERMS).await?;
    let original = building_simple_attributes(&namespace, 1)?;
    create_entity_with_link(&broker, &vocabulary, &original).await?;
    let client = broker.with_context(vocabulary.context())?;
    let id = namespace.urn("Building", "Nonexistent")?;
    let fragment = pisa_tower_located_at_pisa(&namespace, "Nonexistent")?.into_fragment();
    let answer = client.with_responses().entities().merge(&id, &fragment).send().await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
