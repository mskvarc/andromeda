//! Clause C.11: an Attribute carrying a digital signature.
//!
//! The clause's `Store` Entity signs one Attribute -- its `address` Property --
//! with an `ngsildproof` sub-Property whose value is a W3C Data Integrity
//! proof. What the broker has to do is carry that structure faithfully: it is
//! not the signer and not the verifier, but a proof it alters or drops is a
//! proof that no longer verifies.
//!
//! The printed example is not valid JSON: the comma after the `address`
//! Property's `value` object is missing. The corpus repairs it, and the repair
//! is recorded in `docs/annex-c-corpus.md`.

use crate::{
    clean_tenant,
    comparison::{ContextMember, EntityOrder, assert_matches_example, corpus},
    context_link,
    create_entity,
    example_context::hosted_contexts,
    get_json,
};
use pretty_assertions::assert_eq;
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};

/// The signed Store exactly as clause C.11 prints it, with the comma restored.
const STORE: &str = include_str!("corpus/c-11-signed-store.json");

/// Seed the signed Store and answer the `@context` link it is written with.
async fn seed_signed_store(http: &Client) -> String {
    let link = context_link(hosted_contexts().url("store-context.jsonld").as_str());
    let mut store = corpus(STORE);
    if let Some(members) = store.as_object_mut() {
        members.remove("@context");
    }
    create_entity(http, &store, Some(&link)).await;
    link
}

/// Retrieve the signed Store.
async fn retrieve_store(http: &Client, link: &str) -> Value {
    let (status, body) = get_json(http, "/entities/urn:ngsi-ld:Store:002", "application/ld+json", Some(link)).await;
    assert_eq!(status, StatusCode::OK, "Store retrieval failed: {body}");
    body
}

#[tokio::test]
async fn annex_c_11_a_signed_attribute_round_trips_with_its_proof_intact() {
    let environment = clean_tenant().await;
    let link = seed_signed_store(&environment.client()).await;

    let actual = retrieve_store(&environment.client(), &link).await;
    let expected = corpus(STORE);
    assert_matches_example("C.11", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_11_the_proof_carries_the_eddsa_rdfc_2022_crypto_suite() {
    let environment = clean_tenant().await;
    let link = seed_signed_store(&environment.client()).await;

    let actual = retrieve_store(&environment.client(), &link).await;
    let proof = &actual["address"]["ngsildproof"];
    assert_eq!(proof["type"], json!("Property"), "clause 5.2.5 types ngsildproof as a Property");
    assert_eq!(proof["entityIdSealed"], json!("urn:ngsi-ld:Store:002"));
    assert_eq!(proof["entityTypeSealed"], json!("Store"));
    assert_eq!(proof["value"]["type"], json!("DataIntegrityProof"));
    assert_eq!(proof["value"]["cryptosuite"], json!("eddsa-rdfc-2022"));
    assert_eq!(proof["value"]["proofPurpose"], json!("assertionMethod"));
    assert_eq!(proof["value"]["created"], json!("2025-01-27T21:02:24Z"));
    assert_eq!(
        proof["value"]["proofValue"],
        json!("z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS"),
        "a proof value altered in transit is a proof that no longer verifies"
    );
    assert_eq!(
        proof["value"]["verificationMethod"],
        json!("https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT")
    );

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_11_the_atomic_unit_of_signing_is_a_single_attribute() {
    let environment = clean_tenant().await;
    let link = seed_signed_store(&environment.client()).await;

    // "the atomic piece of information that creators can digitally sign in an
    // NGSI-LD ecosystem is each single Attribute of an Entity": the proof rides
    // on `address` and nowhere else, and an Attribute update that leaves
    // `address` alone leaves its proof alone too.
    let unsigned = retrieve_store(&environment.client(), &link).await;
    assert!(
        unsigned["location"].get("ngsildproof").is_none(),
        "only the signed Attribute carries a proof, got {}",
        unsigned["location"]
    );

    let patched = environment
        .client()
        .patch(format!("{}/entities/urn:ngsi-ld:Store:002/attrs", crate::BASE_URL))
        .header("Content-Type", "application/json")
        .header("Link", &link)
        .json(&json!({"location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}}))
        .send()
        .await
        .expect("attribute update request");
    assert_eq!(patched.status(), StatusCode::NO_CONTENT, "updating an unsigned Attribute has to succeed");

    let after = retrieve_store(&environment.client(), &link).await;
    assert_eq!(
        after["address"], unsigned["address"],
        "a change to another Attribute must not disturb the signed one"
    );

    environment.finish().await;
}
