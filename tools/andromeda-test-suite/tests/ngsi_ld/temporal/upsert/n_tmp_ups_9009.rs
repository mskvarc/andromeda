//! A temporal instance carrying a proof reads back with bare seals.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{Lossless, NgsiLdMediaType, RawJson, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Create Temporal Evolution of an Entity whose instance carries an ngsildproof answers 201, and the instance
/// reads back with bare seals
/// Clause: 4.5.2.2, 4.5.8, 5.6.11
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9009_a_temporal_instance_carrying_a_proof_reads_back_with_bare_seals() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_temporal_id = namespace.urn("Building", "temporal")?;
    let building_type = namespace.entity_type("Building")?;
    let building_temporal_evolution = TemporalEntity::builder()
        .id(building_temporal_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses it (an attribute proof), so `temperature` is written as text.
    let temperature: RawJson = format!(r#"[{{"type": "Property", "value": 21.5, "observedAt": "2025-01-01T00:00:00Z", "ngsildproof": {{"type": "Property", "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-temporal", "entityTypeSealed": "Building_{namespace}", "value": {{"type": "DataIntegrityProof", "created": "2025-01-27T21:02:24Z", "verificationMethod": "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT", "cryptosuite": "eddsa-rdfc-2022", "proofPurpose": "assertionMethod", "proofValue": "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS"}}}}}}]"#).parse()?;
    let document = UncheckedDocument::try_from(building_temporal_evolution.render(Lossless::Normalized))?.set_member("temperature", &temperature);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&building_temporal_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_temporal_id)
        .temporal_q(temporal_query::after("2024-01-01T00:00:00Z".parse()?))
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
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "temperature",
            nth(
                0,
                any_attribute()
                    .sealed_to(building_temporal_id.clone())
                    .sealed_type(building_type.clone())
                    .proof_member(["type"], "DataIntegrityProof")
                    .proof_member(["created"], "2025-01-27T21:02:24Z")
                    .proof_member(["verificationMethod"], "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT")
                    .proof_member(["cryptosuite"], "eddsa-rdfc-2022")
                    .proof_member(["proofPurpose"], "assertionMethod")
                    .proof_member(["proofValue"], "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS")
            )
        )
    );
    Ok(())
}
