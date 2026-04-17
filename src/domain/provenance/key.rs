use crate::domain::provenance::error::ProvenanceError;
use ed25519_dalek::VerifyingKey as Ed25519VerifyingKey;
use p256::ecdsa::VerifyingKey as P256VerifyingKey;
use p384::ecdsa::VerifyingKey as P384VerifyingKey;

/// Multicodec prefix for an Ed25519 public key (`0xed 0x01`).
const MULTICODEC_ED25519: [u8; 2] = [0xed, 0x01];
/// Multicodec prefix for a P-256 public key (`0x80 0x24`).
const MULTICODEC_P256: [u8; 2] = [0x80, 0x24];
/// Multicodec prefix for a P-384 public key (`0x81 0x24`).
const MULTICODEC_P384: [u8; 2] = [0x81, 0x24];

/// Public key usable for W3C Data Integrity proof verification.
#[derive(Debug, Clone)]
pub enum VerificationKey {
    /// Ed25519 public key (32 bytes after multicodec prefix).
    Ed25519(Ed25519VerifyingKey),
    /// NIST P-256 public key (compressed SEC1, 33 bytes after prefix).
    EcdsaP256(P256VerifyingKey),
    /// NIST P-384 public key (compressed SEC1, 49 bytes after prefix).
    EcdsaP384(P384VerifyingKey),
}

impl VerificationKey {
    /// Decode a multibase-encoded multicodec-framed public key string.
    ///
    /// The literal must be a W3C Data Integrity compliant
    /// `publicKeyMultibase` value: a `z`-prefixed base58btc payload whose
    /// decoded bytes start with one of the supported multicodec prefixes.
    pub fn from_multibase(value: &str) -> Result<VerificationKey, ProvenanceError> {
        let (_base, bytes) = multibase::decode(value).map_err(ProvenanceError::MultibaseError)?;
        if bytes.len() < 2 {
            return Err(ProvenanceError::KeyError("truncated multicodec header".into()));
        }
        let (prefix, payload) = bytes.split_at(2);
        match prefix {
            p if p == MULTICODEC_ED25519 => {
                let fixed: [u8; 32] = payload
                    .try_into()
                    .map_err(|_| ProvenanceError::KeyError("ed25519 key must be 32 bytes".into()))?;
                let key = Ed25519VerifyingKey::from_bytes(&fixed).map_err(|e| ProvenanceError::KeyError(e.to_string()))?;
                Ok(VerificationKey::Ed25519(key))
            }
            p if p == MULTICODEC_P256 => {
                let key = P256VerifyingKey::from_sec1_bytes(payload).map_err(|e| ProvenanceError::KeyError(e.to_string()))?;
                Ok(VerificationKey::EcdsaP256(key))
            }
            p if p == MULTICODEC_P384 => {
                let key = P384VerifyingKey::from_sec1_bytes(payload).map_err(|e| ProvenanceError::KeyError(e.to_string()))?;
                Ok(VerificationKey::EcdsaP384(key))
            }
            other => Err(ProvenanceError::KeyError(format!("unknown multicodec prefix: {other:?}"))),
        }
    }

    /// Encode this key back into its `z`-prefixed base58btc multibase form.
    ///
    /// Used only by round-trip unit tests in this crate. Production
    /// callers only ever decode keys (see [`VerificationKey::from_multibase`]).
    #[cfg(test)]
    pub fn to_multibase(&self) -> String {
        let mut buffer: Vec<u8> = Vec::with_capacity(64);
        match self {
            VerificationKey::Ed25519(k) => {
                buffer.extend_from_slice(&MULTICODEC_ED25519);
                buffer.extend_from_slice(k.as_bytes());
            }
            VerificationKey::EcdsaP256(k) => {
                buffer.extend_from_slice(&MULTICODEC_P256);
                buffer.extend_from_slice(k.to_sec1_bytes().as_ref());
            }
            VerificationKey::EcdsaP384(k) => {
                buffer.extend_from_slice(&MULTICODEC_P384);
                buffer.extend_from_slice(k.to_sec1_bytes().as_ref());
            }
        }
        multibase::encode(multibase::Base::Base58Btc, buffer)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use ed25519_dalek::SigningKey as Ed25519SigningKey;
    use p256::ecdsa::SigningKey as P256SigningKey;
    use p384::ecdsa::SigningKey as P384SigningKey;

    fn ed25519_test_key() -> Ed25519VerifyingKey {
        let seed: [u8; 32] = [7u8; 32];
        Ed25519SigningKey::from_bytes(&seed).verifying_key()
    }

    fn p256_test_key() -> P256VerifyingKey {
        let bytes: [u8; 32] = [3u8; 32];
        let signing = P256SigningKey::from_bytes((&bytes).into()).expect("p256 key");
        *signing.verifying_key()
    }

    fn p384_test_key() -> P384VerifyingKey {
        let bytes: [u8; 48] = [5u8; 48];
        let signing = P384SigningKey::from_bytes((&bytes).into()).expect("p384 key");
        *signing.verifying_key()
    }

    #[test]
    fn verification_key_multibase_roundtrips_ed25519() {
        let key = VerificationKey::Ed25519(ed25519_test_key());
        let encoded = key.to_multibase();
        let decoded = VerificationKey::from_multibase(&encoded).expect("decode");
        pretty_assertions::assert_eq!(decoded.to_multibase(), encoded);
    }

    #[test]
    fn verification_key_multibase_roundtrips_ecdsa_p256() {
        let key = VerificationKey::EcdsaP256(p256_test_key());
        let encoded = key.to_multibase();
        let decoded = VerificationKey::from_multibase(&encoded).expect("decode");
        pretty_assertions::assert_eq!(decoded.to_multibase(), encoded);
    }

    #[test]
    fn verification_key_multibase_roundtrips_ecdsa_p384() {
        let key = VerificationKey::EcdsaP384(p384_test_key());
        let encoded = key.to_multibase();
        let decoded = VerificationKey::from_multibase(&encoded).expect("decode");
        pretty_assertions::assert_eq!(decoded.to_multibase(), encoded);
    }

    #[test]
    fn verification_key_rejects_unknown_multicodec() {
        let bogus = multibase::encode(multibase::Base::Base58Btc, [0xaa, 0xbb, 0x00, 0x01]);
        assert!(VerificationKey::from_multibase(&bogus).is_err());
    }

    #[test]
    fn verification_key_rejects_invalid_multibase_prefix() {
        assert!(VerificationKey::from_multibase("not-multibase").is_err());
    }

    #[test]
    fn verification_key_rejects_truncated_bytes() {
        let tiny = multibase::encode(multibase::Base::Base58Btc, [0xed]);
        assert!(VerificationKey::from_multibase(&tiny).is_err());
    }
}
