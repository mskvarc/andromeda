//! NGSI-LD Data Integrity verification service (ETSI GS CIM 019 v1.2.1).
//!
//! Verification is opt-in and runs synchronously against an [`Entity`]:
//! each sealed attribute is reconstructed into an atomic entity and
//! dispatched to the matching cryptosuite verifier. The service does
//! not sign — broker-side signing is forbidden by INT-4.

use crate::domain::{
    attribute::{Attribute, AttributeValue},
    entity::Entity,
    provenance::{
        crypto::{CryptoSuiteVerifier, VerificationOutcome, for_cryptosuite},
        error::ProvenanceError,
        key::VerificationKey,
        transform::reconstruct_atomic_entity_from_entity,
    },
    uri::Uri,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::Serialize;
use std::sync::Arc;
use tracing::debug;

/// Result of verifying a single sealed attribute.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SealedAttributeVerification {
    /// Name of the attribute that carried the sealed proof.
    pub attribute: String,
    /// Status classification for the verified attribute.
    pub status: VerificationStatus,
}

/// Classification of a single attribute's verification outcome.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum VerificationStatus {
    /// Signature matched the reconstructed atomic entity under the
    /// resolved verification method.
    Verified,
    /// Signature did not match.
    Invalid {
        /// Machine-readable reason for the failure.
        reason: String,
    },
    /// The verification method URI could not be resolved to a key, or
    /// the key type was incompatible with the declared cryptosuite.
    Unresolvable {
        /// Machine-readable reason for the failure.
        reason: String,
    },
    /// The proof carried an `expires` timestamp that has passed.
    Expired {
        /// Expiry timestamp taken from the proof.
        expires: DateTime<Utc>,
    },
}

/// Async resolver from a `verificationMethod` URI to a
/// [`VerificationKey`]. Implementations may consult static config, DID
/// documents, HTTPS endpoints, or any combination.
#[async_trait]
pub trait VerificationMethodResolver: Send + Sync {
    /// Resolve `method_uri` into a verification key. Return
    /// [`ProvenanceError::KeyError`] if the URI cannot be resolved.
    async fn resolve(&self, method_uri: &Uri) -> Result<VerificationKey, ProvenanceError>;
}

/// In-memory resolver backed by a preconfigured map of
/// verification-method URI to multibase-encoded public key. Primary
/// use: unit tests and static configuration blocks.
#[derive(Debug, Default)]
pub struct StaticResolver {
    entries: DashMap<String, VerificationKey>,
}

impl StaticResolver {
    /// Construct an empty resolver.
    pub fn new() -> StaticResolver {
        StaticResolver { entries: DashMap::new() }
    }

    /// Register a verification-method URI against a decoded
    /// [`VerificationKey`]. Used by unit tests that supply keys
    /// programmatically rather than through the config loader.
    #[cfg(test)]
    pub fn insert(&self, method_uri: String, key: VerificationKey) {
        self.entries.insert(method_uri, key);
    }

    /// Register a verification-method URI against a multibase literal.
    pub fn insert_multibase(&self, method_uri: String, multibase: &str) -> Result<(), ProvenanceError> {
        let key = VerificationKey::from_multibase(multibase)?;
        self.entries.insert(method_uri, key);
        Ok(())
    }
}

#[async_trait]
impl VerificationMethodResolver for StaticResolver {
    async fn resolve(&self, method_uri: &Uri) -> Result<VerificationKey, ProvenanceError> {
        self.entries
            .get(&method_uri.to_string())
            .map(|entry| entry.value().clone())
            .ok_or_else(|| ProvenanceError::KeyError(format!("verification method not registered: {method_uri}")))
    }
}

/// HTTPS-backed resolver that fetches a JSON document at the
/// verification-method URL and reads the `publicKeyMultibase` member.
pub struct HttpsResolver {
    client: reqwest::Client,
}

impl HttpsResolver {
    /// Construct a resolver that uses the supplied reqwest client.
    pub const fn new(client: reqwest::Client) -> HttpsResolver {
        HttpsResolver { client }
    }
}

#[async_trait]
impl VerificationMethodResolver for HttpsResolver {
    async fn resolve(&self, method_uri: &Uri) -> Result<VerificationKey, ProvenanceError> {
        let url = method_uri.to_string();
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ProvenanceError::KeyError(format!("HTTP request failed: {e}")))?;
        let status = response.status();
        if !status.is_success() {
            return Err(ProvenanceError::KeyError(format!("HTTP {status} from {url}")));
        }
        let value: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProvenanceError::KeyError(format!("invalid JSON from {url}: {e}")))?;
        let multibase = value
            .get("publicKeyMultibase")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ProvenanceError::KeyError(format!("no publicKeyMultibase at {url}")))?;
        VerificationKey::from_multibase(multibase)
    }
}

/// Resolver that consults several resolvers in order and returns the
/// first successful key. Used at startup to let static config take
/// precedence over dynamic HTTPS dereference.
pub struct ChainedResolver {
    resolvers: Vec<Arc<dyn VerificationMethodResolver>>,
}

impl ChainedResolver {
    /// Build a chain from an ordered list of delegates.
    pub const fn new(resolvers: Vec<Arc<dyn VerificationMethodResolver>>) -> ChainedResolver {
        ChainedResolver { resolvers }
    }
}

#[async_trait]
impl VerificationMethodResolver for ChainedResolver {
    async fn resolve(&self, method_uri: &Uri) -> Result<VerificationKey, ProvenanceError> {
        let mut last_error: Option<ProvenanceError> = None;
        for resolver in &self.resolvers {
            match resolver.resolve(method_uri).await {
                Ok(key) => return Ok(key),
                Err(err) => last_error = Some(err),
            }
        }
        Err(last_error.unwrap_or_else(|| ProvenanceError::KeyError("no resolvers configured".into())))
    }
}

/// Verification service. Carries an [`Arc<dyn VerificationMethodResolver>`]
/// so callers can plug in static config, HTTPS, or composed resolvers.
pub struct ProvenanceService {
    resolver: Arc<dyn VerificationMethodResolver>,
}

impl ProvenanceService {
    /// Construct a service backed by `resolver`.
    pub const fn new(resolver: Arc<dyn VerificationMethodResolver>) -> ProvenanceService {
        ProvenanceService { resolver }
    }

    /// Verify every sealed attribute carried by `entity`. Attributes
    /// without an `ngsildproof` sub-property are skipped.
    pub async fn verify_entity(&self, entity: &Entity) -> Vec<SealedAttributeVerification> {
        let mut results = Vec::new();
        for (name, value) in &entity.attrs {
            let attribute = match single_attribute(value) {
                Some(a) => a,
                None => continue,
            };
            if !has_ngsildproof(&attribute) {
                continue;
            }
            let status = self.verify_attribute(entity, name).await;
            results.push(SealedAttributeVerification {
                attribute: name.clone(),
                status,
            });
        }
        results
    }

    async fn verify_attribute(&self, entity: &Entity, name: &str) -> VerificationStatus {
        let atomic = match reconstruct_atomic_entity_from_entity(entity, name) {
            Ok(a) => a,
            Err(err) => return VerificationStatus::Invalid { reason: err.to_string() },
        };

        if let Err(ProvenanceError::ProofExpired { expires }) = atomic.proof.value.ensure_not_expired(Utc::now()) {
            return VerificationStatus::Expired { expires };
        }

        let key = match self.resolver.resolve(&atomic.proof.value.verification_method).await {
            Ok(k) => k,
            Err(err) => return VerificationStatus::Unresolvable { reason: err.to_string() },
        };

        let verifier: Box<dyn CryptoSuiteVerifier + Send + Sync> = for_cryptosuite(atomic.proof.value.cryptosuite);
        debug!(
            attribute = name,
            cryptosuite = %verifier.cryptosuite(),
            verification_method = %atomic.proof.value.verification_method,
            "dispatching Data Integrity verifier"
        );
        match verifier.verify(&atomic, &atomic.proof.value, &atomic.proof.value.proof_value, &key) {
            Ok(VerificationOutcome::Valid) => VerificationStatus::Verified,
            Ok(VerificationOutcome::Invalid) => VerificationStatus::Invalid {
                reason: "signature mismatch".into(),
            },
            Err(ProvenanceError::KeyError(reason)) => VerificationStatus::Unresolvable { reason },
            Err(err) => VerificationStatus::Invalid { reason: err.to_string() },
        }
    }
}

fn single_attribute(value: &AttributeValue) -> Option<Attribute> {
    match value {
        AttributeValue::Single(attr) => Some((**attr).clone()),
        AttributeValue::Multiple(attrs) if attrs.len() == 1 => Some(attrs[0].clone()),
        AttributeValue::Multiple(_) | AttributeValue::Raw(_) => None,
    }
}

const fn has_ngsildproof(attribute: &Attribute) -> bool {
    let common = match attribute {
        Attribute::Property(a) => &a.common,
        Attribute::Relationship(a) => &a.common,
        Attribute::VocabProperty(a) => &a.common,
        Attribute::GeoProperty(a) => &a.common,
        Attribute::LanguageProperty(a) => &a.common,
        Attribute::JsonProperty(a) => &a.common,
        Attribute::ListProperty(a) => &a.common,
        Attribute::ListRelationship(a) => &a.common,
    };
    common.ngsildproof.is_some()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use crate::domain::{
        provenance::{
            crypto::rdfc,
            cryptosuite::{Cryptosuite, ProofPurpose},
            ngsild_proof::{NgsildProof, NgsildProofType},
            proof::{DataIntegrityProof, DataIntegrityProofType, ProofValue},
            transform::derive_sealed_attribute,
        },
        uri::Uri,
    };
    use ed25519_dalek::{Signer, SigningKey as Ed25519SigningKey};
    use std::str::FromStr;

    fn sign_atomic_entity(core_value: i32) -> (Entity, VerificationKey) {
        let signing = Ed25519SigningKey::from_bytes(&[7u8; 32]);
        let verifying = signing.verifying_key();
        let placeholder = multibase::encode(multibase::Base::Base58Btc, vec![0_u8; 64]);
        let placeholder_proof = NgsildProof {
            type_: NgsildProofType,
            value: DataIntegrityProof {
                type_: DataIntegrityProofType,
                cryptosuite: Cryptosuite::EddsaRdfc2022,
                created: None,
                verification_method: Uri::from_str("https://example.com/keys/1").unwrap(),
                proof_purpose: ProofPurpose::AssertionMethod,
                proof_value: ProofValue::try_from(placeholder).unwrap(),
                challenge: None,
                expires: None,
            },
            entity_id_sealed: None,
            entity_type_sealed: None,
        };
        let bare_attribute: Attribute = serde_json::from_value(serde_json::json!({
            "type": "Property",
            "value": core_value
        }))
        .unwrap();
        let sealed = derive_sealed_attribute(bare_attribute, placeholder_proof).unwrap();
        let entity_json = serde_json::json!({
            "id": "urn:ngsi-ld:Store:002",
            "type": "Store",
            "address": serde_json::to_value(&sealed).unwrap()
        });
        let mut entity: Entity = serde_json::from_value(entity_json).unwrap();

        let atomic = reconstruct_atomic_entity_from_entity(&entity, "address").unwrap();
        let message = rdfc::signing_input(&atomic, &atomic.proof.value).unwrap();
        let signature = signing.sign(&message);
        let real_proof_value = ProofValue::try_from(multibase::encode(multibase::Base::Base58Btc, signature.to_bytes())).unwrap();

        let new_proof = NgsildProof {
            type_: NgsildProofType,
            value: DataIntegrityProof {
                type_: DataIntegrityProofType,
                cryptosuite: Cryptosuite::EddsaRdfc2022,
                created: None,
                verification_method: Uri::from_str("https://example.com/keys/1").unwrap(),
                proof_purpose: ProofPurpose::AssertionMethod,
                proof_value: real_proof_value,
                challenge: None,
                expires: None,
            },
            entity_id_sealed: None,
            entity_type_sealed: None,
        };
        let resealed = derive_sealed_attribute(atomic.attribute.clone(), new_proof).unwrap();
        entity.attrs.insert("address".to_string(), AttributeValue::Single(Box::new(resealed)));

        (entity, VerificationKey::Ed25519(verifying))
    }

    fn service_with(key: Option<(&str, VerificationKey)>) -> ProvenanceService {
        let resolver = StaticResolver::new();
        if let Some((uri, k)) = key {
            resolver.insert(uri.to_string(), k);
        }
        ProvenanceService::new(Arc::new(resolver))
    }

    #[tokio::test]
    async fn verify_entity_reports_all_attributes_verified_when_proofs_valid() {
        let (entity, key) = sign_atomic_entity(1);
        let service = service_with(Some(("https://example.com/keys/1", key)));
        let results = service.verify_entity(&entity).await;
        pretty_assertions::assert_eq!(results.len(), 1);
        pretty_assertions::assert_eq!(results[0].attribute, "address");
        pretty_assertions::assert_eq!(results[0].status, VerificationStatus::Verified);
    }

    #[tokio::test]
    async fn verify_entity_reports_invalid_when_core_tampered() {
        let (mut entity, key) = sign_atomic_entity(1);
        let service = service_with(Some(("https://example.com/keys/1", key)));
        if let Some(AttributeValue::Single(attr)) = entity.attrs.get_mut("address")
            && let Attribute::Property(p) = attr.as_mut()
        {
            p.value = crate::domain::attribute::common::NgsiLdValue::Json(serde_json::json!(999));
        }
        let results = service.verify_entity(&entity).await;
        pretty_assertions::assert_eq!(results.len(), 1);
        assert!(matches!(results[0].status, VerificationStatus::Invalid { .. }));
    }

    #[tokio::test]
    async fn verify_entity_reports_unresolvable_when_resolver_misses_key() {
        let (entity, _) = sign_atomic_entity(1);
        let service = service_with(None);
        let results = service.verify_entity(&entity).await;
        pretty_assertions::assert_eq!(results.len(), 1);
        assert!(matches!(results[0].status, VerificationStatus::Unresolvable { .. }));
    }

    #[tokio::test]
    async fn verify_entity_reports_expired_when_proof_past_expires_field() {
        let (mut entity, key) = sign_atomic_entity(1);
        if let Some(AttributeValue::Single(attr)) = entity.attrs.get_mut("address")
            && let Attribute::Property(p) = attr.as_mut()
            && let Some(ref mut proof_prop) = p.common.ngsildproof
        {
            // Inject an expiry in the past by rewriting the nested proof JSON.
            let mut json = serde_json::to_value(&**proof_prop).unwrap();
            json["value"]["expires"] = serde_json::json!("2000-01-01T00:00:00Z");
            let new_prop: crate::domain::attribute::Property = serde_json::from_value(json).unwrap();
            **proof_prop = new_prop;
        }
        let service = service_with(Some(("https://example.com/keys/1", key)));
        let results = service.verify_entity(&entity).await;
        pretty_assertions::assert_eq!(results.len(), 1);
        assert!(matches!(results[0].status, VerificationStatus::Expired { .. }));
    }

    #[tokio::test]
    async fn verify_entity_skips_unsealed_attributes() {
        let entity: Entity = serde_json::from_value(serde_json::json!({
            "id": "urn:ngsi-ld:Store:002",
            "type": "Store",
            "plain": { "type": "Property", "value": 1 }
        }))
        .unwrap();
        let service = service_with(None);
        let results = service.verify_entity(&entity).await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn verify_entity_handles_mixed_sealed_and_unsealed() {
        let (mut entity, key) = sign_atomic_entity(1);
        entity.attrs.insert(
            "plain".to_string(),
            AttributeValue::Single(Box::new(serde_json::from_value(serde_json::json!({"type":"Property","value":42})).unwrap())),
        );
        let service = service_with(Some(("https://example.com/keys/1", key)));
        let results = service.verify_entity(&entity).await;
        pretty_assertions::assert_eq!(results.len(), 1);
        pretty_assertions::assert_eq!(results[0].attribute, "address");
    }
}
