//! Appending and merging a proof keeps the seals bare.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{AppendAttributes, MergeEntity},
    },
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType, RawJson, Value},
};

/// Pins: Append Entity Attributes adding an attribute carrying an ngsildproof and Merge Entity adding one to
/// temperature each answer 204, and both proofs read back with bare seals
/// Clause: 4.5.2.2, 5.6.3, 5.6.17
#[gtest]
#[tokio::test]
async fn n_att_app_9006_appending_and_merging_a_proof_keeps_the_seals_bare() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_append_merge_id = namespace.urn("Building", "append-merge")?;
    let building_type = namespace.entity_type("Building")?;
    let building_append_merge = Entity::builder()
        .id(building_append_merge_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_append_merge).send().await?.done()?;
    let fragment = EntityFragment::builder().build()?;
    // The model refuses it (an attribute proof), so `sealed` is written as text.
    let sealed: RawJson = format!(r#"{{"type": "Property", "value": 1, "ngsildproof": {{"type": "Property", "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-append-merge", "entityTypeSealed": "Building_{namespace}", "value": {{"type": "DataIntegrityProof", "created": "2025-01-27T21:02:24Z", "verificationMethod": "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT", "cryptosuite": "eddsa-rdfc-2022", "proofPurpose": "assertionMethod", "proofValue": "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS"}}}}}}"#).parse()?;
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("sealed", &sealed);
    let request = broker
        .unchecked(&AppendAttributes::builder().id(&building_append_merge_id).fragment(&fragment).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_append_merge_id)
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("ngsi-ld:entityIdSealed")));
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("ngsi-ld:entityTypeSealed")));
    expect_that!(
        String::from_utf8_lossy(&received.body),
        not(contains_substring("https://uri.etsi.org/ngsi-ld/entityIdSealed"))
    );
    expect_that!(
        String::from_utf8_lossy(&received.body),
        not(contains_substring("https://uri.etsi.org/ngsi-ld/entityTypeSealed"))
    );
    expect_that!(
        String::from_utf8_lossy(&received.body),
        not(contains_substring("default-context/entityIdSealed"))
    );
    expect_that!(
        String::from_utf8_lossy(&received.body),
        not(contains_substring("default-context/entityTypeSealed"))
    );
    let retrieved = &received.outcome;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "sealed",
            any_attribute()
                .sealed_to(building_append_merge_id.clone())
                .sealed_type(building_type.clone())
                .proof_member(["type"], "DataIntegrityProof")
                .proof_member(["created"], "2025-01-27T21:02:24Z")
                .proof_member(["verificationMethod"], "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT")
                .proof_member(["cryptosuite"], "eddsa-rdfc-2022")
                .proof_member(["proofPurpose"], "assertionMethod")
                .proof_member(["proofValue"], "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS")
        )
    );
    let fragment_2 = EntityFragment::builder().build()?;
    // The model refuses it (an attribute proof), so `temperature` is written as text.
    let temperature: RawJson = format!(r#"{{"type": "Property", "value": 23.5, "ngsildproof": {{"type": "Property", "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-append-merge", "entityTypeSealed": "Building_{namespace}", "value": {{"type": "DataIntegrityProof", "created": "2025-01-27T21:02:24Z", "verificationMethod": "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT", "cryptosuite": "eddsa-rdfc-2022", "proofPurpose": "assertionMethod", "proofValue": "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS"}}}}}}"#).parse()?;
    let document_2 = UncheckedDocument::try_from(fragment_2.render(Lossless::Normalized))?.set_member("temperature", &temperature);
    let request_2 = broker
        .unchecked(&MergeEntity::builder().id(&building_append_merge_id).fragment(&fragment_2).build())?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    let received_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_append_merge_id)
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received_2.body), not(contains_substring("ngsi-ld:entityIdSealed")));
    expect_that!(String::from_utf8_lossy(&received_2.body), not(contains_substring("ngsi-ld:entityTypeSealed")));
    expect_that!(
        String::from_utf8_lossy(&received_2.body),
        not(contains_substring("https://uri.etsi.org/ngsi-ld/entityIdSealed"))
    );
    expect_that!(
        String::from_utf8_lossy(&received_2.body),
        not(contains_substring("https://uri.etsi.org/ngsi-ld/entityTypeSealed"))
    );
    expect_that!(
        String::from_utf8_lossy(&received_2.body),
        not(contains_substring("default-context/entityIdSealed"))
    );
    expect_that!(
        String::from_utf8_lossy(&received_2.body),
        not(contains_substring("default-context/entityTypeSealed"))
    );
    let retrieved_2 = &received_2.outcome;
    let entity_2 = retrieved_2.body();
    expect_that!(
        entity_2,
        has_attribute(
            "temperature",
            any_attribute()
                .sealed_to(building_append_merge_id.clone())
                .sealed_type(building_type.clone())
                .proof_member(["type"], "DataIntegrityProof")
                .proof_member(["created"], "2025-01-27T21:02:24Z")
                .proof_member(["verificationMethod"], "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT")
                .proof_member(["cryptosuite"], "eddsa-rdfc-2022")
                .proof_member(["proofPurpose"], "assertionMethod")
                .proof_member(["proofValue"], "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS")
        )
    );
    Ok(())
}
