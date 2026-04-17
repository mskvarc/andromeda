use serde::{Deserialize, Serialize};
use strum::{Display, EnumCount, EnumIter, EnumString};

/// W3C Data Integrity cryptosuite identifiers required by ETSI GS CIM 019
/// v1.2.1 clause 5.1.1. Wire form is the canonical kebab-case suite name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString, EnumIter, EnumCount, Serialize, Deserialize)]
pub enum Cryptosuite {
    /// Ed25519 signature over JSON-LD canonicalised via RDFC-1.0.
    #[strum(serialize = "eddsa-rdfc-2022")]
    #[serde(rename = "eddsa-rdfc-2022")]
    EddsaRdfc2022,
    /// Ed25519 signature over JCS-canonicalised JSON.
    #[strum(serialize = "eddsa-jcs-2022")]
    #[serde(rename = "eddsa-jcs-2022")]
    EddsaJcs2022,
    /// ECDSA P-256 / P-384 signature over JSON-LD canonicalised via RDFC-1.0.
    #[strum(serialize = "ecdsa-rdfc-2019")]
    #[serde(rename = "ecdsa-rdfc-2019")]
    EcdsaRdfc2019,
    /// ECDSA P-256 / P-384 signature over JCS-canonicalised JSON.
    #[strum(serialize = "ecdsa-jcs-2019")]
    #[serde(rename = "ecdsa-jcs-2019")]
    EcdsaJcs2019,
}

/// Proof purpose identifier from the W3C Data Integrity vocabulary.
/// Wire form is camelCase to match the `proofPurpose` literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString, EnumIter, Serialize, Deserialize)]
pub enum ProofPurpose {
    /// Assertion of data authenticity.
    #[strum(serialize = "assertionMethod")]
    #[serde(rename = "assertionMethod")]
    AssertionMethod,
    /// Proof of possession of private key for authentication.
    #[strum(serialize = "authentication")]
    #[serde(rename = "authentication")]
    Authentication,
    /// Proof used during capability invocation.
    #[strum(serialize = "capabilityInvocation")]
    #[serde(rename = "capabilityInvocation")]
    CapabilityInvocation,
    /// Proof used during capability delegation.
    #[strum(serialize = "capabilityDelegation")]
    #[serde(rename = "capabilityDelegation")]
    CapabilityDelegation,
    /// Used as a key agreement mechanism.
    #[strum(serialize = "keyAgreement")]
    #[serde(rename = "keyAgreement")]
    KeyAgreement,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    #[test]
    fn cryptosuite_display_matches_kebab_case_for_all_variants() {
        pretty_assertions::assert_eq!(Cryptosuite::EddsaRdfc2022.to_string(), "eddsa-rdfc-2022");
        pretty_assertions::assert_eq!(Cryptosuite::EddsaJcs2022.to_string(), "eddsa-jcs-2022");
        pretty_assertions::assert_eq!(Cryptosuite::EcdsaRdfc2019.to_string(), "ecdsa-rdfc-2019");
        pretty_assertions::assert_eq!(Cryptosuite::EcdsaJcs2019.to_string(), "ecdsa-jcs-2019");
    }

    #[test]
    fn cryptosuite_parse_rejects_unknown_string() {
        assert!(Cryptosuite::from_str("bogus-suite-2099").is_err());
    }

    #[test]
    fn proof_purpose_roundtrips_camel_case_for_all_variants() {
        for variant in ProofPurpose::iter() {
            let s = variant.to_string();
            let parsed = ProofPurpose::from_str(&s).expect("roundtrip");
            pretty_assertions::assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn proof_purpose_parse_rejects_unknown_string() {
        assert!(ProofPurpose::from_str("nope").is_err());
    }

    #[test]
    fn cryptosuite_serde_preserves_kebab_case() {
        let value = serde_json::to_value(Cryptosuite::EcdsaRdfc2019).expect("serialize");
        pretty_assertions::assert_eq!(value, serde_json::json!("ecdsa-rdfc-2019"));
        let back: Cryptosuite = serde_json::from_value(value).expect("deserialize");
        pretty_assertions::assert_eq!(back, Cryptosuite::EcdsaRdfc2019);
    }

    #[test]
    fn cryptosuite_count_matches_spec() {
        pretty_assertions::assert_eq!(Cryptosuite::COUNT, 4);
    }
}
