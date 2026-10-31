//! A batch creation keeps the seals bare.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::BatchCreate},
    model::{Entity, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Batch Entity Creation of an Entity whose attribute carries an ngsildproof answers 201, and the proof reads
/// back with bare seals
/// Clause: 4.5.2.2, 5.6.7
#[gtest]
#[tokio::test]
async fn n_bat_cre_9005_a_batch_creation_keeps_the_seals_bare() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_batch_id = namespace.urn("Building", "batch")?;
    let building_type = namespace.entity_type("Building")?;
    let building_batch = Entity::builder().id(building_batch_id.clone()).entity_type(building_type.clone()).build()?;
    // The model refuses it (an attribute proof), so `sealed` is written as text.
    let sealed: RawJson = format!(r#"{{"type": "Property", "value": 21.5, "ngsildproof": {{"type": "Property", "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-batch", "entityTypeSealed": "Building_{namespace}", "value": {{"type": "DataIntegrityProof", "created": "2025-01-27T21:02:24Z", "verificationMethod": "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT", "cryptosuite": "eddsa-rdfc-2022", "proofPurpose": "assertionMethod", "proofValue": "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS"}}}}}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&building_batch, Lossless::Normalized).set_member("sealed", &sealed);
    let entities = [building_batch];
    let request = broker.unchecked(&BatchCreate::builder().entities(&entities).build())?.documents(&[document]);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_batch_id)
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
                .sealed_to(building_batch_id.clone())
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
