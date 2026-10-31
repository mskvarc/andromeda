//! A relationship named ngsildproof admits no seal.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, RawJson},
};

/// Pins: Create Entity with a sub-attribute named ngsildproof that is a Relationship, normalized with
/// `entityIdSealed` or concise with `entityTypeSealed`, answers 400 `BadRequestData`
/// Clause: 4.5.2.2, 4.5.3.2, 4.5.3.3, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9023_a_relationship_named_ngsildproof_admits_no_seal() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_relationship_proof_normalized_id = namespace.urn("Building", "relationship-proof-normalized")?;
    let building_type = namespace.entity_type("Building")?;
    let building_relationship_proof_concise_id = namespace.urn("Building", "relationship-proof-concise")?;
    let building_relationship_proof_normalized = Entity::builder()
        .id(building_relationship_proof_normalized_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (an ngsildproof must be a Property, found a Relationship at sealed.ngsildproof), so `sealed` is written as text.
    let sealed: RawJson = format!(r#"{{"type": "Property", "value": 21.5, "ngsildproof": {{"type": "Relationship", "object": "urn:ngsi-ld:Proof:{namespace}-1", "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-relationship-proof-normalized"}}}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&building_relationship_proof_normalized, Lossless::Normalized).set_member("sealed", &sealed);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_relationship_proof_normalized).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_relationship_proof_concise = Entity::builder()
        .id(building_relationship_proof_concise_id)
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (an attribute in normalized representation needs a `type` member at sealed.ngsildproof), so `sealed` is written as text.
    let sealed_2: RawJson = format!(
        r#"{{"type": "Property", "value": 21.5, "ngsildproof": {{"object": "urn:ngsi-ld:Proof:{namespace}-1", "entityTypeSealed": "Building_{namespace}"}}}}"#
    )
    .parse()?;
    let document_2 = UncheckedDocument::from_entity(&building_relationship_proof_concise, Lossless::Normalized).set_member("sealed", &sealed_2);
    let request_2 = broker
        .unchecked(&CreateEntity::builder().entity(&building_relationship_proof_concise).build())?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
