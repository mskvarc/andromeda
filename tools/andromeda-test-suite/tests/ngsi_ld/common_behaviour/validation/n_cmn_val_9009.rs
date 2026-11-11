//! A seal on a relationship is refused on creation.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, RawJson},
};

/// Pins: Create Entity with `entityIdSealed` or `entityTypeSealed` on a normalized or a concise Relationship
/// answers 400 `BadRequestData` in each of the four cases, and an entity carrying both seals inside the ngsildproof
/// sub-Property of a Property answers 201
/// Clause: 4.5.3.2, 4.5.3.3, 4.5.2.2, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_cmn_val_9009_a_seal_on_a_relationship_is_refused_on_creation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_normalized_entity_id_sealed_id = namespace.urn("Building", "normalized-entityIdSealed")?;
    let building_type = namespace.entity_type("Building")?;
    let building_concise_entity_id_sealed_id = namespace.urn("Building", "concise-entityIdSealed")?;
    let building_normalized_entity_type_sealed_id = namespace.urn("Building", "normalized-entityTypeSealed")?;
    let building_concise_entity_type_sealed_id = namespace.urn("Building", "concise-entityTypeSealed")?;
    let building_proof_id = namespace.urn("Building", "proof")?;
    let building_normalized_entity_id_sealed = Entity::builder()
        .id(building_normalized_entity_id_sealed_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `entityIdSealed` is not allowed here at belongsTo), so `belongsTo` is written as text.
    let belongs_to: RawJson =
        format!(r#"{{"type": "Relationship", "object": "urn:ngsi-ld:Site:{namespace}-1", "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-sealed"}}"#)
            .parse()?;
    let document = UncheckedDocument::from_entity(&building_normalized_entity_id_sealed, Lossless::Normalized).set_member("belongsTo", &belongs_to);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_normalized_entity_id_sealed).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_concise_entity_id_sealed = Entity::builder()
        .id(building_concise_entity_id_sealed_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (an attribute in normalized representation needs a `type` member at belongsTo), so `belongsTo` is written as text.
    let belongs_to_2: RawJson =
        format!(r#"{{"object": "urn:ngsi-ld:Site:{namespace}-1", "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-sealed"}}"#).parse()?;
    let document_2 = UncheckedDocument::from_entity(&building_concise_entity_id_sealed, Lossless::Normalized).set_member("belongsTo", &belongs_to_2);
    let request_2 = broker
        .unchecked(&CreateEntity::builder().entity(&building_concise_entity_id_sealed).build())?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_normalized_entity_type_sealed = Entity::builder()
        .id(building_normalized_entity_type_sealed_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (member `entityTypeSealed` is not allowed here at belongsTo), so `belongsTo` is written as text.
    let belongs_to_3: RawJson =
        format!(r#"{{"type": "Relationship", "object": "urn:ngsi-ld:Site:{namespace}-1", "entityTypeSealed": "Building_{namespace}"}}"#).parse()?;
    let document_3 = UncheckedDocument::from_entity(&building_normalized_entity_type_sealed, Lossless::Normalized).set_member("belongsTo", &belongs_to_3);
    let request_3 = broker
        .unchecked(&CreateEntity::builder().entity(&building_normalized_entity_type_sealed).build())?
        .document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_concise_entity_type_sealed = Entity::builder()
        .id(building_concise_entity_type_sealed_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (an attribute in normalized representation needs a `type` member at belongsTo), so `belongsTo` is written as text.
    let belongs_to_4: RawJson = format!(r#"{{"object": "urn:ngsi-ld:Site:{namespace}-1", "entityTypeSealed": "Building_{namespace}"}}"#).parse()?;
    let document_4 = UncheckedDocument::from_entity(&building_concise_entity_type_sealed, Lossless::Normalized).set_member("belongsTo", &belongs_to_4);
    let request_4 = broker
        .unchecked(&CreateEntity::builder().entity(&building_concise_entity_type_sealed).build())?
        .document(&document_4);
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_proof = Entity::builder().id(building_proof_id).entity_type(building_type.clone()).build()?;
    // The model refuses it (an attribute proof), so `temperature` is written as text.
    let temperature: RawJson = format!(r#"{{"type": "Property", "value": 21.5, "ngsildproof": {{"type": "Property", "value": {{"type": "DataIntegrityProof", "created": "2025-01-27T21:02:24Z", "verificationMethod": "https://example.edu/issuers/565049#z6MkwXG2WjeQnN", "cryptosuite": "eddsa-jcs-2022", "proofPurpose": "assertionMethod", "proofValue": "z2aB"}}, "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-proof", "entityTypeSealed": "Building_{namespace}"}}}}"#).parse()?;
    let document_5 = UncheckedDocument::from_entity(&building_proof, Lossless::Normalized).set_member("temperature", &temperature);
    let request_5 = broker
        .unchecked(&CreateEntity::builder().entity(&building_proof).build())?
        .document(&document_5);
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_5, answered(StatusCode::CREATED));
    Ok(())
}
