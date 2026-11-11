//! A data-integrity seal as a `sub_attribute` element stores it (ETSI GS CIM
//! 009 v1.9.1 clause 4.5.2.2): its two seals in fields of their own, and its
//! W3C Data Integrity proof as the `data_integrity_proof` composite in
//! `value_proof`, the proof's `@id` and further members in the stored JSON of
//! the structured value codec.
//!
//! A seal is only ever a sub-attribute, so only an element has these fields;
//! on every other element they are NULL.

use crate::{stored_json::StoredJson, stored_members::StoredInstanceError};
use andromeda_ngsi_ld_entity::proof::{cryptosuite::Cryptosuite, proof_value::ProofValue};
use andromeda_ngsi_ld_lexicon::{
    identifier::Identifier,
    time::{date_time::NgsiLdDateTime, spelled_date_time::SpelledDateTime},
    uri::Uri,
};
use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
use chrono::{DateTime, Utc};
use serde_json::value::RawValue;
use sqlx::{Type, types::Json};

/// The seals and proof of one data-integrity seal.
#[derive(Debug, PartialEq)]
pub struct StoredProof {
    /// `entityIdSealed`.
    pub entity_id: Identifier,
    /// `entityTypeSealed`, as `entity_type_names` keys in written order;
    /// never empty.
    pub entity_type_keys: Vec<EntityTypeNameKey>,
    /// `created`, with the width it was written at.
    pub created: SpelledDateTime,
    /// `verificationMethod`.
    pub verification_method: Uri,
    /// `cryptosuite`.
    pub cryptosuite: Cryptosuite,
    /// `proofPurpose`, its expanded IRI.
    pub proof_purpose: Uri,
    /// `proofValue`.
    pub proof_value: ProofValue,
    /// The proof's `@id` and further members, `None` when it has none.
    pub members: Option<StoredJson>,
}

/// The `data_integrity_proof` composite, field for field in declaration
/// order.
///
/// A composite field cannot be declared `not null`, so each is optional here,
/// and a stored proof missing one is refused as it is read.
#[derive(Debug, Type)]
#[sqlx(type_name = "data_integrity_proof")]
pub struct DataIntegrityProofRecord {
    /// `created`, the instant.
    pub created: Option<DateTime<Utc>>,
    /// `created_digits`, the fractional-second digits `created` is written
    /// with.
    pub created_digits: Option<i16>,
    /// `cryptosuite`.
    pub cryptosuite: Option<String>,
    /// `proof_purpose`, the expanded IRI of `proofPurpose`.
    pub proof_purpose: Option<String>,
    /// `verification_method`.
    pub verification_method: Option<String>,
    /// `proof_value`, as written.
    pub proof_value: Option<String>,
    /// `further_members`, the proof's `@id` and further members.
    pub further_members: Option<Json<Box<RawValue>>>,
}

/// The seal fields of one `sub_attribute` element, in the composite's
/// declaration order.
#[derive(Debug, Default)]
pub struct ProofFields {
    /// `seal_entity_id`.
    pub seal_entity_id: Option<String>,
    /// `seal_entity_type_keys`.
    pub seal_entity_type_keys: Option<Vec<i32>>,
    /// `value_proof`.
    pub value_proof: Option<DataIntegrityProofRecord>,
}

impl ProofFields {
    /// Whether every field is NULL, as on an element that holds no seal.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.seal_entity_id.is_none() && self.seal_entity_type_keys.is_none() && self.value_proof.is_none()
    }

    /// The seal the fields hold.
    ///
    /// # Errors
    ///
    /// Returns [`StoredInstanceError::SealFields`] when a required field is
    /// NULL or holds no value of its member.
    pub fn read(self) -> Result<StoredProof, StoredInstanceError> {
        let seal = || StoredInstanceError::SealFields;
        let entity_type_keys: Vec<EntityTypeNameKey> = self.seal_entity_type_keys.ok_or_else(seal)?.into_iter().map(EntityTypeNameKey::from).collect();
        if entity_type_keys.is_empty() {
            return Err(seal());
        }
        let proof = self.value_proof.ok_or_else(seal)?;
        let instant = NgsiLdDateTime::normalise(proof.created.ok_or_else(seal)?).map_err(|_| seal())?;
        let digits = u8::try_from(proof.created_digits.ok_or_else(seal)?).map_err(|_| seal())?;
        Ok(StoredProof {
            entity_id: self.seal_entity_id.ok_or_else(seal)?.parse().map_err(|_| seal())?,
            entity_type_keys,
            created: SpelledDateTime::new(instant, digits).map_err(|_| seal())?,
            verification_method: proof.verification_method.ok_or_else(seal)?.parse().map_err(|_| seal())?,
            cryptosuite: proof.cryptosuite.ok_or_else(seal)?.parse().map_err(|_| seal())?,
            proof_purpose: proof.proof_purpose.ok_or_else(seal)?.parse().map_err(|_| seal())?,
            proof_value: proof.proof_value.ok_or_else(seal)?.parse().map_err(|_| seal())?,
            members: proof.further_members.map(StoredJson::from),
        })
    }
}

impl From<StoredProof> for ProofFields {
    fn from(proof: StoredProof) -> ProofFields {
        ProofFields {
            seal_entity_id: Some(proof.entity_id.as_str().to_owned()),
            seal_entity_type_keys: Some(proof.entity_type_keys.into_iter().map(EntityTypeNameKey::get).collect()),
            value_proof: Some(DataIntegrityProofRecord {
                created: Some(proof.created.instant().get()),
                created_digits: Some(i16::from(proof.created.fractional_digits())),
                cryptosuite: Some(proof.cryptosuite.as_str().to_owned()),
                proof_purpose: Some(proof.proof_purpose.as_str().to_owned()),
                verification_method: Some(proof.verification_method.as_str().to_owned()),
                proof_value: Some(proof.proof_value.as_str().to_owned()),
                further_members: proof.members.map(StoredJson::into_json),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        stored_members::StoredInstanceError,
        stored_proof::{ProofFields, StoredProof},
    };
    use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;

    fn proof() -> StoredProof {
        StoredProof {
            entity_id: "urn:ngsi-ld:Store:002".parse().expect("an identifier"),
            entity_type_keys: vec![EntityTypeNameKey::from(7)],
            created: "2025-01-27T21:02:24.50Z".parse().expect("a DateTime"),
            verification_method: "https://example.edu/issuers/565049#key".parse().expect("a URI"),
            cryptosuite: "eddsa-rdfc-2022".parse().expect("a suite"),
            proof_purpose: "https://w3id.org/security#assertionMethod".parse().expect("a URI"),
            proof_value: "z3XrH3".parse().expect("a signature"),
            members: None,
        }
    }

    /// The width `created` was written at is stored beside its instant, so it
    /// reads back with the digits it was signed with.
    #[test]
    fn a_seal_reads_back_from_its_fields() {
        let fields = ProofFields::from(proof());
        pretty_assertions::assert_eq!(fields.value_proof.as_ref().and_then(|stored| stored.created_digits), Some(2));
        pretty_assertions::assert_eq!(fields.read().expect("a seal").created.to_string(), "2025-01-27T21:02:24.50Z");
        pretty_assertions::assert_eq!(ProofFields::from(proof()).read().expect("a seal"), proof());
    }

    #[test]
    fn a_seal_missing_a_field_is_refused() {
        let mut fields = ProofFields::from(proof());
        if let Some(stored) = fields.value_proof.as_mut() {
            stored.proof_value = None;
        }
        assert!(matches!(fields.read(), Err(StoredInstanceError::SealFields)));
        let mut proofless = ProofFields::from(proof());
        proofless.value_proof = None;
        assert!(matches!(proofless.read(), Err(StoredInstanceError::SealFields)));
        let mut typeless = ProofFields::from(proof());
        typeless.seal_entity_type_keys = Some(Vec::new());
        assert!(matches!(typeless.read(), Err(StoredInstanceError::SealFields)));
    }

    #[test]
    fn an_element_holding_no_seal_has_no_seal_field() {
        assert!(ProofFields::default().is_empty());
        assert!(!ProofFields::from(proof()).is_empty());
    }
}
