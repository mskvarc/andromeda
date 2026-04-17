use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::domain::provenance::cryptosuite::Cryptosuite;

/// Errors produced while handling NGSI-LD Data Integrity proofs
/// (ETSI GS CIM 019 v1.2.1 clause 5).
#[derive(Debug, Error)]
pub enum ProvenanceError {
    /// The entity did not have exactly one attribute after head / tail
    /// stripping, violating the Atomic Entity shape (CIM 019 clause 5.2.2).
    #[error("atomic entity must contain exactly one attribute, found {0}")]
    InvalidAttributeCount(usize),

    /// The attribute had no `ngsildproof` sub-property attached.
    #[error("attribute is missing the ngsildproof sub-property")]
    MissingNgsildProof,

    /// The `ngsildproof` sub-property was structurally invalid.
    #[error("ngsildproof sub-property is invalid: {reason}")]
    InvalidNgsildProof {
        /// Human-readable reason.
        reason: String,
    },

    /// The embedded `proofValue` multibase literal was invalid.
    #[error("proofValue is not a valid multibase literal")]
    InvalidProofValue,

    /// The `type` member of a DataIntegrityProof did not equal the
    /// literal `"DataIntegrityProof"`.
    #[error("proof type must be DataIntegrityProof")]
    InvalidProofType,

    /// The outer ngsildproof sub-property was not a Property.
    #[error("ngsildproof sub-property must have type Property")]
    InvalidNgsildProofType,

    /// The cryptosuite named by the proof is not supported by this build.
    #[error("unsupported cryptosuite: {0}")]
    UnsupportedCryptosuite(Cryptosuite),

    /// The proof carried an `expires` literal that has already passed.
    #[error("proof expired at {expires}")]
    ProofExpired {
        /// The `expires` timestamp carried on the proof.
        expires: DateTime<Utc>,
    },

    /// RDFC canonicalisation failed for the atomic entity.
    #[error("canonicalization failed: {0}")]
    CanonicalizationFailed(String),

    /// Serialising the atomic entity for hashing failed.
    #[error("serialization failed: {0}")]
    SerializationFailed(String),

    /// Decoding or parsing the verification key failed.
    #[error("key error: {0}")]
    KeyError(String),

    /// Multibase decoding failed on key or proof-value bytes.
    #[error(transparent)]
    MultibaseError(#[from] multibase::Error),
}
