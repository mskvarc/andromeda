//! NGSI-LD Data Integrity (ETSI GS CIM 019 v1.2.1) integration tests.
//!
//! Exercises the end-to-end HTTP plumbing of `verify=true`, the
//! `ngsildproofVerification` sidecar, status classification
//! (Verified, Invalid, Unresolvable, Expired), and rendering-mode
//! preservation of the `ngsildproof` sub-property (INT-3).
//!
//! Requires a running broker on `127.0.0.1:1056` with
//! `provenance.https_resolver_enabled = true` so the test-local HTTP
//! key server is reachable by the broker.
//!
//! Naming convention: `<category>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use axum::{Json, Router, routing::get};
use ed25519_dalek::{Signer, SigningKey};
use p256::ecdsa::{Signature as P256Signature, SigningKey as P256SigningKey};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use tokio::sync::oneshot;
use uuid::Uuid;

const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

fn client() -> Client {
    Client::new()
}

fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

async fn post_entity(c: &Client, body: &Value) -> StatusCode {
    c.post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("post_entity")
        .status()
}

async fn get_entity(c: &Client, id: &str, query: &str) -> (StatusCode, Value) {
    let qs = if query.is_empty() { String::new() } else { format!("?{query}") };
    let resp = c
        .get(format!("{BASE_URL}/entities/{id}{qs}"))
        .header("Accept", "application/ld+json")
        .header(
            "Link",
            format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
        )
        .send()
        .await
        .expect("get_entity");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

async fn delete_entity(c: &Client, id: &str) {
    let _ = c.delete(format!("{BASE_URL}/entities/{id}")).send().await;
}

async fn patch_attr(c: &Client, id: &str, attr: &str, body: &Value) -> StatusCode {
    c.patch(format!("{BASE_URL}/entities/{id}/attrs/{attr}"))
        .header("Content-Type", "application/merge-patch+json")
        .header(
            "Link",
            format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
        )
        .json(body)
        .send()
        .await
        .expect("patch_attr")
        .status()
}

// ---------------------------------------------------------------------------
// Ed25519 key server
// ---------------------------------------------------------------------------

const ED25519_MULTICODEC: [u8; 2] = [0xed, 0x01];
const P256_MULTICODEC: [u8; 2] = [0x80, 0x24];

fn pubkey_multibase(signing: &SigningKey) -> String {
    let mut buf = Vec::with_capacity(34);
    buf.extend_from_slice(&ED25519_MULTICODEC);
    buf.extend_from_slice(signing.verifying_key().as_bytes());
    multibase::encode(multibase::Base::Base58Btc, &buf)
}

fn p256_pubkey_multibase(signing: &P256SigningKey) -> String {
    let mut buf = Vec::with_capacity(35);
    buf.extend_from_slice(&P256_MULTICODEC);
    buf.extend_from_slice(signing.verifying_key().to_sec1_bytes().as_ref());
    multibase::encode(multibase::Base::Base58Btc, &buf)
}

struct KeyServer {
    addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
}

impl KeyServer {
    async fn start(public_key_multibase: String) -> KeyServer {
        let app = Router::new().route(
            "/key",
            get(move || {
                let mb = public_key_multibase.clone();
                async move { Json(json!({ "publicKeyMultibase": mb })) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.expect("bind key server");
        let addr = listener.local_addr().expect("local addr");
        let (tx, rx) = oneshot::channel();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await;
        });
        KeyServer { addr, shutdown: Some(tx) }
    }

    fn url(&self) -> String {
        // Broker runs in Docker; reach host via host.docker.internal.
        // Override with TEST_HOST_FROM_BROKER for non-Docker setups.
        let host = std::env::var("TEST_HOST_FROM_BROKER").unwrap_or_else(|_| "host.docker.internal".into());
        format!("http://{}:{}/key", host, self.addr.port())
    }
}

impl Drop for KeyServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

// ---------------------------------------------------------------------------
// JCS signing pipeline (mirrors src/domain/provenance/crypto/jcs.rs)
// ---------------------------------------------------------------------------

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    let out = h.finalize();
    let mut arr = [0_u8; 32];
    arr.copy_from_slice(&out);
    arr
}

fn jcs_sha256(value: &Value) -> [u8; 32] {
    sha256(&serde_jcs::to_vec(value).expect("jcs"))
}

/// System-managed attribute fields that the broker stamps after receipt
/// and that `reconstruct_atomic_entity` strips before computing the
/// canonicalised input. Mirrors `AttributeSystemFields` in
/// `src/domain/attribute/common.rs`.
const SYSTEM_FIELDS: [&str; 4] = ["createdAt", "modifiedAt", "instanceId", "deletedAt"];

fn strip_system_fields(value: &mut Value) {
    if let Some(obj) = value.as_object_mut() {
        for k in SYSTEM_FIELDS {
            obj.remove(k);
        }
    }
}

/// Compute the broker-side `signing_input` bytes for a sealed attribute,
/// using the broker's stored representation of the attribute. The
/// approach: GET the entity (verify=false) and rebuild the unsigned
/// atomic-entity JSON from what the broker actually serialises, with
/// system fields and ngsildproof stripped (matching the verifier path).
fn proof_config_from_stored(stored: &Value, attribute_name: &str) -> Value {
    let mut cfg = stored
        .get(attribute_name)
        .and_then(|attribute| attribute.get("ngsildproof"))
        .and_then(|proof| proof.get("value"))
        .cloned()
        .expect("stored entity must contain ngsildproof.value for sealed attribute");
    if let Value::Object(ref mut m) = cfg {
        m.remove("proofValue");
    }
    cfg
}

fn broker_signing_input(stored: &Value, attribute_name: &str, proof_config: &Value) -> Vec<u8> {
    let mut attr = stored.get(attribute_name).cloned().expect("stored entity must contain the sealed attribute");
    strip_system_fields(&mut attr);
    let sealed_head_id = attr.get("ngsildproof").and_then(|proof| proof.get("entityIdSealed")).cloned();
    let sealed_head_type = attr.get("ngsildproof").and_then(|proof| proof.get("entityTypeSealed")).cloned();
    if let Some(obj) = attr.as_object_mut() {
        obj.remove("ngsildproof");
    }
    let mut unsigned = serde_json::Map::new();
    unsigned.insert("id".to_string(), sealed_head_id.unwrap_or_else(|| stored["id"].clone()));
    unsigned.insert("type".to_string(), sealed_head_type.unwrap_or_else(|| stored["type"].clone()));
    unsigned.insert(attribute_name.to_string(), attr);
    let hash_data = jcs_sha256(&Value::Object(unsigned));

    let mut cfg = proof_config.clone();
    if let Value::Object(ref mut m) = cfg {
        m.remove("proofValue");
    }
    let hash_proof = jcs_sha256(&cfg);

    let mut buf = Vec::with_capacity(64);
    buf.extend_from_slice(&hash_proof);
    buf.extend_from_slice(&hash_data);
    buf
}

fn resealed_entity_from_stored(stored: &Value, id: &str, attribute_name: &str, proof_value: &str) -> Value {
    let mut attribute = stored.get(attribute_name).cloned().expect("stored entity must contain the sealed attribute");
    strip_system_fields(&mut attribute);
    let mut entity = json!({
        "id": id,
        "type": stored["type"].clone(),
        attribute_name: attribute,
        "@context": CORE_CONTEXT
    });
    let proof_value_slot = entity
        .get_mut(attribute_name)
        .and_then(Value::as_object_mut)
        .and_then(|attribute_obj| attribute_obj.get_mut("ngsildproof"))
        .and_then(Value::as_object_mut)
        .and_then(|proof_obj| proof_obj.get_mut("value"))
        .and_then(Value::as_object_mut)
        .and_then(|proof_value_obj| proof_value_obj.get_mut("proofValue"))
        .expect("sealed attribute must carry ngsildproof.value.proofValue");
    *proof_value_slot = Value::String(proof_value.to_string());
    entity
}

// ---------------------------------------------------------------------------
// Entity fixture helpers
// ---------------------------------------------------------------------------

fn build_sealed_entity_with_suite(id: &str, cryptosuite: &str, verification_method: &str, proof_value: &str) -> Value {
    json!({
        "id": id,
        "type": "Store",
        "address": {
            "type": "Property",
            "value": 42,
            "ngsildproof": {
                "type": "Property",
                "value": {
                    "type": "DataIntegrityProof",
                    "cryptosuite": cryptosuite,
                    "verificationMethod": verification_method,
                    "proofPurpose": "assertionMethod",
                    "proofValue": proof_value,
                }
            }
        },
        "@context": CORE_CONTEXT
    })
}

fn build_sealed_entity(id: &str, verification_method: &str, proof_value: &str) -> Value {
    build_sealed_entity_with_suite(id, "eddsa-jcs-2022", verification_method, proof_value)
}

fn find_verifications(body: &Value) -> Option<&Vec<Value>> {
    body.get("ngsildproofVerification").and_then(Value::as_array)
}

fn address_proof(body: &Value) -> Option<&Value> {
    body.get("address").and_then(|a| a.get("ngsildproof"))
}

// ---------------------------------------------------------------------------
// Tests: verify flag wiring
// ---------------------------------------------------------------------------

#[tokio::test]
async fn verify_false_default_omits_verification_sidecar() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Store:diFalseDefault");
    let entity = build_sealed_entity(&id, "https://example.invalid/key", "z3bogusProofValue");

    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);
    let (status, body) = get_entity(&c, &id, "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.get("ngsildproofVerification").is_none(),
        "default GET must not include verification sidecar"
    );

    delete_entity(&c, &id).await;
}

#[tokio::test]
async fn verify_true_empty_array_for_unsealed_entity() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Store:diEmpty");
    let entity = json!({
        "id": &id,
        "type": "Store",
        "address": {"type": "Property", "value": 42},
        "@context": CORE_CONTEXT,
    });

    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);
    let (status, body) = get_entity(&c, &id, "verify=true").await;
    assert_eq!(status, StatusCode::OK);
    let verifications = find_verifications(&body).expect("ngsildproofVerification key present");
    assert!(
        verifications.is_empty(),
        "unsealed entity yields empty verification array, got {verifications:?}"
    );

    delete_entity(&c, &id).await;
}

#[tokio::test]
async fn verify_true_unresolvable_for_unknown_verification_method() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Store:diUnresolvable");
    // Use a non-routable https URL so HttpsResolver's fetch fails.
    let entity = build_sealed_entity(
        &id,
        "https://invalid.test.local.nowhere/key",
        &multibase::encode(multibase::Base::Base58Btc, vec![0_u8; 64]),
    );

    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);
    let (_, body) = get_entity(&c, &id, "verify=true").await;
    let verifications = find_verifications(&body).expect("verification array");
    assert_eq!(verifications.len(), 1);
    let entry = &verifications[0];
    assert_eq!(entry["attribute"], "address");
    assert_eq!(entry["status"]["kind"], "unresolvable", "entry={entry}");

    delete_entity(&c, &id).await;
}

#[tokio::test]
async fn verify_true_invalid_for_wrong_signature() {
    let c = client();
    let signing = SigningKey::from_bytes(&[0x42_u8; 32]);
    let server = KeyServer::start(pubkey_multibase(&signing)).await;
    let method_uri = server.url();

    let id = unique_id("urn:ngsi-ld:Store:diInvalid");
    // 64-byte signature of all zeros — valid encoding, wrong signature.
    let bogus_sig = multibase::encode(multibase::Base::Base58Btc, vec![0_u8; 64]);
    let entity = build_sealed_entity(&id, &method_uri, &bogus_sig);

    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);
    let (_, body) = get_entity(&c, &id, "verify=true").await;
    let verifications = find_verifications(&body).expect("verification array");
    assert_eq!(verifications.len(), 1);
    let entry = &verifications[0];
    assert_eq!(entry["attribute"], "address");
    assert_eq!(entry["status"]["kind"], "invalid", "entry={entry}");

    delete_entity(&c, &id).await;
}

#[tokio::test]
async fn verify_true_verified_for_valid_signature() {
    let c = client();
    let signing = SigningKey::from_bytes(&[0x17_u8; 32]);
    let server = KeyServer::start(pubkey_multibase(&signing)).await;
    let method_uri = server.url();

    let id = unique_id("urn:ngsi-ld:Store:diVerified");

    // Two-pass: POST with placeholder signature, GET to learn the broker's
    // stored serialisation, sign that exact form, DELETE + re-POST with the
    // valid signature. Avoids guessing fields the broker round-trips.
    let placeholder = multibase::encode(multibase::Base::Base58Btc, vec![0_u8; 64]);
    let entity = build_sealed_entity(&id, &method_uri, &placeholder);
    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);
    let (_, stored) = get_entity(&c, &id, "").await;
    let proof_cfg = proof_config_from_stored(&stored, "address");

    let signing_input = broker_signing_input(&stored, "address", &proof_cfg);
    let sig = signing.sign(&signing_input);
    let proof_value = multibase::encode(multibase::Base::Base58Btc, sig.to_bytes());

    let _ = c.delete(format!("{BASE_URL}/entities/{id}")).send().await;
    let entity = resealed_entity_from_stored(&stored, &id, "address", &proof_value);
    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);

    let (_, body) = get_entity(&c, &id, "verify=true").await;
    let verifications = find_verifications(&body).expect("verification array");
    assert_eq!(verifications.len(), 1);
    let entry = &verifications[0];
    assert_eq!(entry["attribute"], "address");
    assert_eq!(entry["status"]["kind"], "verified", "entry={entry}");

    delete_entity(&c, &id).await;
}

#[tokio::test]
async fn verify_true_expired_when_proof_expires_in_past() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Store:diExpired");
    let proof_value = multibase::encode(multibase::Base::Base58Btc, vec![0_u8; 64]);
    let entity = json!({
        "id": &id,
        "type": "Store",
        "address": {
            "type": "Property",
            "value": 42,
            "ngsildproof": {
                "type": "Property",
                "value": {
                    "type": "DataIntegrityProof",
                    "cryptosuite": "eddsa-jcs-2022",
                    "verificationMethod": "https://invalid.test.local.nowhere/key",
                    "proofPurpose": "assertionMethod",
                    "expires": "2000-01-01T00:00:00Z",
                    "proofValue": proof_value,
                }
            }
        },
        "@context": CORE_CONTEXT,
    });

    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);
    let (_, body) = get_entity(&c, &id, "verify=true").await;
    let verifications = find_verifications(&body).expect("verification array");
    assert_eq!(verifications.len(), 1);
    let entry = &verifications[0];
    assert_eq!(entry["attribute"], "address");
    assert_eq!(entry["status"]["kind"], "expired", "entry={entry}");

    delete_entity(&c, &id).await;
}

#[tokio::test]
async fn verify_true_mixed_sealed_and_unsealed_reports_only_sealed() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Store:diMixed");
    let proof_value = multibase::encode(multibase::Base::Base58Btc, vec![0_u8; 64]);
    let entity = json!({
        "id": &id,
        "type": "Store",
        "address": {
            "type": "Property",
            "value": 42,
            "ngsildproof": {
                "type": "Property",
                "value": {
                    "type": "DataIntegrityProof",
                    "cryptosuite": "eddsa-jcs-2022",
                    "verificationMethod": "https://invalid.test.local.nowhere/key",
                    "proofPurpose": "assertionMethod",
                    "proofValue": proof_value,
                }
            }
        },
        "name": {"type": "Property", "value": "Acme"},
        "@context": CORE_CONTEXT,
    });

    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);
    let (_, body) = get_entity(&c, &id, "verify=true").await;
    let verifications = find_verifications(&body).expect("verification array");
    assert_eq!(verifications.len(), 1, "only sealed attribute yields entry, got {verifications:?}");
    assert_eq!(verifications[0]["attribute"], "address");

    delete_entity(&c, &id).await;
}

// ---------------------------------------------------------------------------
// Tests: ngsildproof preservation across render modes and operations (INT-3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn normalized_render_preserves_ngsildproof_sub_property() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Store:diNormalized");
    let entity = build_sealed_entity(&id, "https://example.invalid/key", "z3bogus");
    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);

    let (_, body) = get_entity(&c, &id, "").await;
    let proof = address_proof(&body).expect("ngsildproof present on address attribute");
    assert_eq!(proof["type"], "Property");
    assert_eq!(proof["value"]["type"], "DataIntegrityProof");
    assert_eq!(proof["value"]["cryptosuite"], "eddsa-jcs-2022");

    delete_entity(&c, &id).await;
}

#[tokio::test]
async fn concise_render_preserves_ngsildproof_sub_property() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Store:diConcise");
    let entity = build_sealed_entity(&id, "https://example.invalid/key", "z3bogus");
    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);

    let (_, body) = get_entity(&c, &id, "options=concise").await;
    // In concise mode, Property envelopes unwrap, so ngsildproof surfaces
    // with the inner DataIntegrityProof shape directly.
    let proof = address_proof(&body).expect("ngsildproof present");
    let cryptosuite = proof
        .get("cryptosuite")
        .or_else(|| proof.pointer("/value/cryptosuite"))
        .and_then(Value::as_str)
        .expect("cryptosuite present in concise proof");
    assert_eq!(cryptosuite, "eddsa-jcs-2022");

    delete_entity(&c, &id).await;
}

#[tokio::test]
async fn simplified_render_drops_ngsildproof_acknowledged_int3_loss() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Store:diSimplified");
    let entity = build_sealed_entity(&id, "https://example.invalid/key", "z3bogus");
    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);

    let (_, body) = get_entity(&c, &id, "options=keyValues").await;
    let address = body.get("address").expect("address field");
    assert!(
        address.get("ngsildproof").is_none(),
        "simplified form (keyValues) drops ngsildproof per INT-3 acknowledged loss, got {address:?}"
    );

    delete_entity(&c, &id).await;
}

#[tokio::test]
async fn merge_patch_other_attribute_preserves_ngsildproof_on_sealed() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Store:diMergePreserve");
    let proof_value = multibase::encode(multibase::Base::Base58Btc, vec![0_u8; 64]);
    let entity = json!({
        "id": &id,
        "type": "Store",
        "address": {
            "type": "Property",
            "value": 42,
            "ngsildproof": {
                "type": "Property",
                "value": {
                    "type": "DataIntegrityProof",
                    "cryptosuite": "eddsa-jcs-2022",
                    "verificationMethod": "https://example.invalid/key",
                    "proofPurpose": "assertionMethod",
                    "proofValue": proof_value,
                }
            }
        },
        "name": {"type": "Property", "value": "Acme"},
        "@context": CORE_CONTEXT,
    });

    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);
    let status = patch_attr(&c, &id, "name", &json!({"type": "Property", "value": "Globex"})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, body) = get_entity(&c, &id, "").await;
    assert!(address_proof(&body).is_some(), "ngsildproof on sealed attribute survives unrelated merge patch");

    delete_entity(&c, &id).await;
}

// ---------------------------------------------------------------------------
// Tests: verify query parameter parsing
// ---------------------------------------------------------------------------

async fn raw(c: &Client, method: Method, path: &str) -> StatusCode {
    c.request(method, format!("{BASE_URL}{path}"))
        .header("Accept", "application/ld+json")
        .header(
            "Link",
            format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
        )
        .send()
        .await
        .expect("raw")
        .status()
}

#[tokio::test]
async fn verify_param_case_insensitive_true_triggers_verification() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Store:diCase");
    let entity = json!({
        "id": &id,
        "type": "Store",
        "address": {"type": "Property", "value": 42},
        "@context": CORE_CONTEXT,
    });
    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);

    let (_, body) = get_entity(&c, &id, "verify=TRUE").await;
    assert!(
        body.get("ngsildproofVerification").is_some(),
        "verify=TRUE (uppercase) must also trigger verification"
    );

    delete_entity(&c, &id).await;
}

#[tokio::test]
async fn verify_param_false_explicit_omits_sidecar() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Store:diExplicitFalse");
    let entity = json!({
        "id": &id,
        "type": "Store",
        "address": {"type": "Property", "value": 42},
        "@context": CORE_CONTEXT,
    });
    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);

    let (_, body) = get_entity(&c, &id, "verify=false").await;
    assert!(body.get("ngsildproofVerification").is_none(), "verify=false must not attach sidecar");

    delete_entity(&c, &id).await;
}

#[tokio::test]
async fn verify_param_unknown_value_yields_400() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Store:diInvalidVerify");
    let entity = json!({
        "id": &id,
        "type": "Store",
        "address": {"type": "Property", "value": 42},
        "@context": CORE_CONTEXT,
    });
    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);

    let status = raw(&c, Method::GET, &format!("/entities/{id}?verify=notabool")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "non-boolean verify value must be rejected");

    delete_entity(&c, &id).await;
}

// ---------------------------------------------------------------------------
// Tests: ecdsa-jcs-2019 cryptosuite (P-256)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ecdsa_jcs_2019_verified_for_valid_signature() {
    use p256::ecdsa::signature::Signer;
    let c = client();
    let signing = P256SigningKey::from_bytes(&[0x33_u8; 32].into()).expect("p256 signing key");
    let server = KeyServer::start(p256_pubkey_multibase(&signing)).await;
    let method_uri = server.url();

    let id = unique_id("urn:ngsi-ld:Store:diEcdsaJcsOk");

    let placeholder = multibase::encode(multibase::Base::Base58Btc, vec![0_u8; 64]);
    let entity = build_sealed_entity_with_suite(&id, "ecdsa-jcs-2019", &method_uri, &placeholder);
    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);
    let (_, stored) = get_entity(&c, &id, "").await;
    let proof_cfg = proof_config_from_stored(&stored, "address");

    let signing_input = broker_signing_input(&stored, "address", &proof_cfg);
    let sig: P256Signature = signing.sign(&signing_input);
    let proof_value = multibase::encode(multibase::Base::Base58Btc, sig.to_bytes());

    let _ = c.delete(format!("{BASE_URL}/entities/{id}")).send().await;
    let entity = resealed_entity_from_stored(&stored, &id, "address", &proof_value);
    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);

    let (_, body) = get_entity(&c, &id, "verify=true").await;
    let v = find_verifications(&body).expect("array");
    assert_eq!(v.len(), 1);
    assert_eq!(v[0]["status"]["kind"], "verified", "entry={:?}", v[0]);

    delete_entity(&c, &id).await;
}

#[tokio::test]
async fn ecdsa_jcs_2019_invalid_for_wrong_signature() {
    let c = client();
    let signing = P256SigningKey::from_bytes(&[0x44_u8; 32].into()).expect("p256 signing key");
    let server = KeyServer::start(p256_pubkey_multibase(&signing)).await;
    let method_uri = server.url();

    let id = unique_id("urn:ngsi-ld:Store:diEcdsaJcsBad");
    let bogus = multibase::encode(multibase::Base::Base58Btc, vec![0_u8; 64]);
    let entity = build_sealed_entity_with_suite(&id, "ecdsa-jcs-2019", &method_uri, &bogus);

    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);
    let (_, body) = get_entity(&c, &id, "verify=true").await;
    let v = find_verifications(&body).expect("array");
    assert_eq!(v.len(), 1);
    assert_eq!(v[0]["status"]["kind"], "invalid", "entry={:?}", v[0]);

    delete_entity(&c, &id).await;
}

#[tokio::test]
async fn ecdsa_jcs_2019_unresolvable_for_unknown_method() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Store:diEcdsaJcsUnres");
    let bogus = multibase::encode(multibase::Base::Base58Btc, vec![0_u8; 64]);
    let entity = build_sealed_entity_with_suite(&id, "ecdsa-jcs-2019", "https://invalid.test.local.nowhere/key", &bogus);

    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);
    let (_, body) = get_entity(&c, &id, "verify=true").await;
    let v = find_verifications(&body).expect("array");
    assert_eq!(v.len(), 1);
    assert_eq!(v[0]["status"]["kind"], "unresolvable", "entry={:?}", v[0]);

    delete_entity(&c, &id).await;
}

// ---------------------------------------------------------------------------
// Tests: eddsa-rdfc-2022 cryptosuite (Ed25519 over RDFC-1.0 hash)
//
// RDFC canonicalisation is non-trivial to reproduce in tests; we cover the
// dispatch / status classification (the canonicalisation pipeline itself
// is exercised by the 78 provenance unit tests). A `Verified` end-to-end
// test for RDFC suites is out of scope here.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn eddsa_rdfc_2022_invalid_for_wrong_signature() {
    let c = client();
    let signing = SigningKey::from_bytes(&[0x55_u8; 32]);
    let server = KeyServer::start(pubkey_multibase(&signing)).await;
    let method_uri = server.url();

    let id = unique_id("urn:ngsi-ld:Store:diEdRdfcBad");
    let bogus = multibase::encode(multibase::Base::Base58Btc, vec![0_u8; 64]);
    let entity = build_sealed_entity_with_suite(&id, "eddsa-rdfc-2022", &method_uri, &bogus);

    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);
    let (_, body) = get_entity(&c, &id, "verify=true").await;
    let v = find_verifications(&body).expect("array");
    assert_eq!(v.len(), 1);
    assert_eq!(v[0]["status"]["kind"], "invalid", "entry={:?}", v[0]);

    delete_entity(&c, &id).await;
}

#[tokio::test]
async fn eddsa_rdfc_2022_unresolvable_for_unknown_method() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Store:diEdRdfcUnres");
    let bogus = multibase::encode(multibase::Base::Base58Btc, vec![0_u8; 64]);
    let entity = build_sealed_entity_with_suite(&id, "eddsa-rdfc-2022", "https://invalid.test.local.nowhere/key", &bogus);

    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);
    let (_, body) = get_entity(&c, &id, "verify=true").await;
    let v = find_verifications(&body).expect("array");
    assert_eq!(v.len(), 1);
    assert_eq!(v[0]["status"]["kind"], "unresolvable", "entry={:?}", v[0]);

    delete_entity(&c, &id).await;
}

#[tokio::test]
async fn eddsa_rdfc_2022_expired_when_proof_expires_in_past() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Store:diEdRdfcExp");
    let bogus = multibase::encode(multibase::Base::Base58Btc, vec![0_u8; 64]);
    let entity = json!({
        "id": &id,
        "type": "Store",
        "address": {
            "type": "Property",
            "value": 42,
            "ngsildproof": {
                "type": "Property",
                "value": {
                    "type": "DataIntegrityProof",
                    "cryptosuite": "eddsa-rdfc-2022",
                    "verificationMethod": "https://invalid.test.local.nowhere/key",
                    "proofPurpose": "assertionMethod",
                    "expires": "2000-01-01T00:00:00Z",
                    "proofValue": bogus,
                }
            }
        },
        "@context": CORE_CONTEXT,
    });

    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);
    let (_, body) = get_entity(&c, &id, "verify=true").await;
    let v = find_verifications(&body).expect("array");
    assert_eq!(v.len(), 1);
    assert_eq!(v[0]["status"]["kind"], "expired", "entry={:?}", v[0]);

    delete_entity(&c, &id).await;
}

// ---------------------------------------------------------------------------
// Tests: ecdsa-rdfc-2019 cryptosuite (P-256 over RDFC-1.0 hash)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ecdsa_rdfc_2019_invalid_for_wrong_signature() {
    let c = client();
    let signing = P256SigningKey::from_bytes(&[0x66_u8; 32].into()).expect("p256 signing key");
    let server = KeyServer::start(p256_pubkey_multibase(&signing)).await;
    let method_uri = server.url();

    let id = unique_id("urn:ngsi-ld:Store:diEcRdfcBad");
    let bogus = multibase::encode(multibase::Base::Base58Btc, vec![0_u8; 64]);
    let entity = build_sealed_entity_with_suite(&id, "ecdsa-rdfc-2019", &method_uri, &bogus);

    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);
    let (_, body) = get_entity(&c, &id, "verify=true").await;
    let v = find_verifications(&body).expect("array");
    assert_eq!(v.len(), 1);
    assert_eq!(v[0]["status"]["kind"], "invalid", "entry={:?}", v[0]);

    delete_entity(&c, &id).await;
}

#[tokio::test]
async fn ecdsa_rdfc_2019_unresolvable_for_unknown_method() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Store:diEcRdfcUnres");
    let bogus = multibase::encode(multibase::Base::Base58Btc, vec![0_u8; 64]);
    let entity = build_sealed_entity_with_suite(&id, "ecdsa-rdfc-2019", "https://invalid.test.local.nowhere/key", &bogus);

    assert_eq!(post_entity(&c, &entity).await, StatusCode::CREATED);
    let (_, body) = get_entity(&c, &id, "verify=true").await;
    let v = find_verifications(&body).expect("array");
    assert_eq!(v.len(), 1);
    assert_eq!(v[0]["status"]["kind"], "unresolvable", "entry={:?}", v[0]);

    delete_entity(&c, &id).await;
}
