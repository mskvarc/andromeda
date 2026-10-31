//! A local temporal query renders an instance carrying a proof.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    bare_seals::bare_seals,
    broker,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{Lossless, NgsiLdMediaType, RawJson, evolution::TemporalEntity},
    query::{Locality, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with local=true, an id and a window and no other selection answers
/// 200 with that Entity and its instance with bare seals
/// Clause: 4.5.2.2, 5.5.13, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9011_a_local_temporal_query_renders_an_instance_carrying_a_proof() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_local_query_id = namespace.urn("Building", "local-query")?;
    let building_type = namespace.entity_type("Building")?;
    let building_local_query_evolution = TemporalEntity::builder()
        .id(building_local_query_id.clone())
        .entity_type(building_type.clone())
        .build()?;
    // The model refuses an attribute proof, so `temperature` is written as text.
    let temperature: RawJson = format!(r#"[{{"type": "Property", "value": 21.5, "observedAt": "2025-01-01T00:00:00Z", "ngsildproof": {{"type": "Property", "entityIdSealed": "urn:ngsi-ld:Building:{namespace}-local-query", "entityTypeSealed": "Building_{namespace}", "value": {{"type": "DataIntegrityProof", "created": "2025-01-27T21:02:24Z", "verificationMethod": "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT", "cryptosuite": "eddsa-rdfc-2022", "proofPurpose": "assertionMethod", "proofValue": "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS"}}}}}}]"#).parse()?;
    let document = UncheckedDocument::try_from(building_local_query_evolution.render(Lossless::Normalized))?.set_member("temperature", &temperature);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&building_local_query_evolution).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2024-01-01T00:00:00Z".parse()?))
        .locality(Locality::Local)
        .ids(vec![building_local_query_id.clone()])
        .send()
        .await?;
    expect_that!(&received.body, bare_seals());
    let page = &received.outcome;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_local_query_id)));
    expect_that!(
        page.items().first(),
        some(has_evolution(
            "temperature",
            nth(
                0,
                any_attribute()
                    .sealed_to(building_local_query_id.clone())
                    .sealed_type(building_type.clone())
                    .proof_member(["type"], "DataIntegrityProof")
                    .proof_member(["created"], "2025-01-27T21:02:24Z")
                    .proof_member(["verificationMethod"], "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT")
                    .proof_member(["cryptosuite"], "eddsa-rdfc-2022")
                    .proof_member(["proofPurpose"], "assertionMethod")
                    .proof_member(["proofValue"], "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS")
            )
        ))
    );
    Ok(())
}
