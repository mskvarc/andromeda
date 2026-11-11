//! A seal on a relationship is refused per entity.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, partial_outcome::reported, report_matcher::batch_error};
use googletest::prelude::*;
use ngsi_ld::{
    http::{BatchOperationResult, UncheckedDocument, operation::BatchMerge},
    model::{Entity, EntityFragment, ErrorType, Lossless, RawJson, Value},
};

/// Pins: Batch Entity Merge of four entities each carrying `entityIdSealed` or `entityTypeSealed` on a normalized
/// or concise Relationship, beside one carrying both seals inside ngsildproof, answers 207 with a `BadRequestData`
/// error against each of the four and the fifth in success, the five having been created first
/// Clause: 5.6.20.4, 4.5.3.2, 4.5.3.3, 4.5.2.2
#[gtest]
#[tokio::test]
async fn n_bat_mrg_9001_a_seal_on_a_relationship_is_refused_per_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_normalized_entity_id_sealed_id = namespace.urn("Building", "normalized-entityIdSealed")?;
    let building_type = namespace.entity_type("Building")?;
    let building_concise_entity_id_sealed_id = namespace.urn("Building", "concise-entityIdSealed")?;
    let building_normalized_entity_type_sealed_id = namespace.urn("Building", "normalized-entityTypeSealed")?;
    let building_concise_entity_type_sealed_id = namespace.urn("Building", "concise-entityTypeSealed")?;
    let building_proof_id = namespace.urn("Building", "proof")?;
    let entities = [
        Entity::builder()
            .id(building_normalized_entity_id_sealed_id.clone())
            .entity_type(building_type.clone())
            .property("temperature", Value::Number("21.5".parse()?))
            .build()?,
        Entity::builder()
            .id(building_concise_entity_id_sealed_id.clone())
            .entity_type(building_type.clone())
            .property("temperature", Value::Number("21.5".parse()?))
            .build()?,
        Entity::builder()
            .id(building_normalized_entity_type_sealed_id.clone())
            .entity_type(building_type.clone())
            .property("temperature", Value::Number("21.5".parse()?))
            .build()?,
        Entity::builder()
            .id(building_concise_entity_type_sealed_id.clone())
            .entity_type(building_type.clone())
            .property("temperature", Value::Number("21.5".parse()?))
            .build()?,
        Entity::builder()
            .id(building_proof_id.clone())
            .entity_type(building_type.clone())
            .property("temperature", Value::Number("21.5".parse()?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let building_normalized_entity_id_sealed_fragment =
        EntityFragment::for_entity(building_normalized_entity_id_sealed_id.clone(), building_type.clone()).build()?;
    // The model refuses it (member `entityIdSealed` is not allowed here at belongsTo), so `belongsTo` is written as text.
    let belongs_to: RawJson =
        format!(r#"{{"type": "Relationship", "object": "urn:ngsi-ld:Site:{namespace}-1", "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-sealed"}}"#)
            .parse()?;
    let document =
        UncheckedDocument::try_from(building_normalized_entity_id_sealed_fragment.render(Lossless::Normalized))?.set_member("belongsTo", &belongs_to);
    let building_concise_entity_id_sealed_fragment = EntityFragment::for_entity(building_concise_entity_id_sealed_id.clone(), building_type.clone()).build()?;
    // The model refuses it (an attribute in normalized representation needs a `type` member at belongsTo), so `belongsTo` is written as text.
    let belongs_to_2: RawJson =
        format!(r#"{{"object": "urn:ngsi-ld:Site:{namespace}-1", "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-sealed"}}"#).parse()?;
    let document_2 =
        UncheckedDocument::try_from(building_concise_entity_id_sealed_fragment.render(Lossless::Normalized))?.set_member("belongsTo", &belongs_to_2);
    let building_normalized_entity_type_sealed_fragment =
        EntityFragment::for_entity(building_normalized_entity_type_sealed_id.clone(), building_type.clone()).build()?;
    // The model refuses it (member `entityTypeSealed` is not allowed here at belongsTo), so `belongsTo` is written as text.
    let belongs_to_3: RawJson =
        format!(r#"{{"type": "Relationship", "object": "urn:ngsi-ld:Site:{namespace}-1", "entityTypeSealed": "Building_{namespace}"}}"#).parse()?;
    let document_3 =
        UncheckedDocument::try_from(building_normalized_entity_type_sealed_fragment.render(Lossless::Normalized))?.set_member("belongsTo", &belongs_to_3);
    let building_concise_entity_type_sealed_fragment =
        EntityFragment::for_entity(building_concise_entity_type_sealed_id.clone(), building_type.clone()).build()?;
    // The model refuses it (an attribute in normalized representation needs a `type` member at belongsTo), so `belongsTo` is written as text.
    let belongs_to_4: RawJson = format!(r#"{{"object": "urn:ngsi-ld:Site:{namespace}-1", "entityTypeSealed": "Building_{namespace}"}}"#).parse()?;
    let document_4 =
        UncheckedDocument::try_from(building_concise_entity_type_sealed_fragment.render(Lossless::Normalized))?.set_member("belongsTo", &belongs_to_4);
    let building_proof_fragment = EntityFragment::for_entity(building_proof_id.clone(), building_type.clone()).build()?;
    // The model refuses it (an attribute proof), so `temperature` is written as text.
    let temperature: RawJson = format!(r#"{{"type": "Property", "value": 21.5, "ngsildproof": {{"type": "Property", "value": {{"type": "DataIntegrityProof", "created": "2025-01-27T21:02:24Z", "verificationMethod": "https://example.edu/issuers/565049#z6MkwXG2WjeQnN", "cryptosuite": "eddsa-jcs-2022", "proofPurpose": "assertionMethod", "proofValue": "z2aB"}}, "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-proof", "entityTypeSealed": "Building_{namespace}"}}}}"#).parse()?;
    let document_5 = UncheckedDocument::try_from(building_proof_fragment.render(Lossless::Normalized))?.set_member("temperature", &temperature);
    let fragments = [
        building_normalized_entity_id_sealed_fragment,
        building_concise_entity_id_sealed_fragment,
        building_normalized_entity_type_sealed_fragment,
        building_concise_entity_type_sealed_fragment,
        building_proof_fragment,
    ];
    let request = broker
        .unchecked(&BatchMerge::builder().fragments(&fragments).build())?
        .documents(&[document, document_2, document_3, document_4, document_5]);
    let answer = broker.send_unchecked(request).await;
    let report: BatchOperationResult = reported(answer)?;
    expect_that!(&report.success, contains(eq(&building_proof_id)));
    expect_that!(&report.errors, not(contains(batch_error().entity(&building_proof_id))));
    expect_that!(&report.errors, len(eq(4)));
    expect_that!(
        &report.errors,
        contains(
            batch_error()
                .entity(&building_normalized_entity_id_sealed_id)
                .problem(ErrorType::BadRequestData)
        )
    );
    expect_that!(
        &report.errors,
        contains(batch_error().entity(&building_concise_entity_id_sealed_id).problem(ErrorType::BadRequestData))
    );
    expect_that!(
        &report.errors,
        contains(
            batch_error()
                .entity(&building_normalized_entity_type_sealed_id)
                .problem(ErrorType::BadRequestData)
        )
    );
    expect_that!(
        &report.errors,
        contains(batch_error().entity(&building_concise_entity_type_sealed_id).problem(ErrorType::BadRequestData))
    );
    Ok(())
}
