//! One attribute instance (clauses 4.5.1, 4.5.5).
//!
//! The instance's identity, its `datasetId`, is the key it is held under in
//! [`AttributeInstances`](crate::attribute_instances::AttributeInstances), not
//! a member of the instance itself: two instances with one identity cannot be
//! held at all.

use crate::{attributes::Attributes, date_time_text::DateTimeText, family_value::FamilyValue, proof_seals::ProofSeals, system_instant::SystemInstant};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, identifier::Identifier};
use typed_builder::TypedBuilder;

/// One attribute instance: its family and value, its temporal members and its
/// sub-attributes.
#[derive(Clone, Debug, PartialEq, TypedBuilder)]
pub struct Instance {
    /// The family and its value.
    value: FamilyValue,
    /// `observedAt` (clause 4.5.1), as written.
    #[builder(default = None)]
    observed_at: Option<DateTimeText>,
    /// `expiresAt` (clause 4.22), as written.
    #[builder(default = None)]
    expires_at: Option<DateTimeText>,
    /// `createdAt` (clause 4.8), stamped by the broker.
    #[builder(default = None)]
    created_at: Option<SystemInstant>,
    /// `modifiedAt` (clause 4.8), stamped by the broker.
    #[builder(default = None)]
    modified_at: Option<SystemInstant>,
    /// `instanceId` (clause 4.5.7), on Temporal Evolution instances.
    #[builder(default = None)]
    evolution_id: Option<Identifier>,
    /// The `entityIdSealed` and `entityTypeSealed` members of clause 4.5.2.2.
    #[builder(default = ProofSeals::default())]
    proof_seals: ProofSeals,
    /// The sub-attributes (clause 4.5.2), recursively.
    #[builder(default = Attributes::new())]
    sub_attributes: Attributes,
}

impl Instance {
    /// The family and its value.
    #[must_use]
    pub const fn value(&self) -> &FamilyValue {
        &self.value
    }

    /// The family.
    #[must_use]
    pub const fn family(&self) -> AttributeType {
        self.value.family()
    }

    /// `observedAt`, when present.
    #[must_use]
    pub const fn observed_at(&self) -> Option<&DateTimeText> {
        self.observed_at.as_ref()
    }

    /// `expiresAt`, when present.
    #[must_use]
    pub const fn expires_at(&self) -> Option<&DateTimeText> {
        self.expires_at.as_ref()
    }

    /// `createdAt`, when stamped.
    #[must_use]
    pub const fn created_at(&self) -> Option<SystemInstant> {
        self.created_at
    }

    /// `modifiedAt`, when stamped.
    #[must_use]
    pub const fn modified_at(&self) -> Option<SystemInstant> {
        self.modified_at
    }

    /// `instanceId`, when present.
    #[must_use]
    pub const fn instance_id(&self) -> Option<&Identifier> {
        self.evolution_id.as_ref()
    }

    /// The data-integrity seals.
    #[must_use]
    pub const fn proof_seals(&self) -> &ProofSeals {
        &self.proof_seals
    }

    /// The sub-attributes.
    #[must_use]
    pub const fn sub_attributes(&self) -> &Attributes {
        &self.sub_attributes
    }

    /// Replace the family and its value.
    pub fn set_value(&mut self, value: FamilyValue) {
        self.value = value;
    }

    /// The family value, for a mutation that keeps the family.
    pub const fn value_mut(&mut self) -> &mut FamilyValue {
        &mut self.value
    }

    /// Replace the data-integrity seals.
    pub fn set_proof_seals(&mut self, proof_seals: ProofSeals) {
        self.proof_seals = proof_seals;
    }

    /// Replace `observedAt`.
    pub fn set_observed_at(&mut self, observed_at: Option<DateTimeText>) {
        self.observed_at = observed_at;
    }

    /// Replace `expiresAt`.
    pub fn set_expires_at(&mut self, expires_at: Option<DateTimeText>) {
        self.expires_at = expires_at;
    }

    /// Stamp `createdAt`.
    pub const fn set_created_at(&mut self, created_at: Option<SystemInstant>) {
        self.created_at = created_at;
    }

    /// Stamp `modifiedAt`.
    pub const fn set_modified_at(&mut self, modified_at: Option<SystemInstant>) {
        self.modified_at = modified_at;
    }

    /// Replace `instanceId`.
    pub fn set_instance_id(&mut self, instance_id: Option<Identifier>) {
        self.evolution_id = instance_id;
    }

    /// The sub-attributes, for a mutation.
    pub const fn sub_attributes_mut(&mut self) -> &mut Attributes {
        &mut self.sub_attributes
    }

    /// Whether two instances agree on everything but their system times
    /// (clause 4.8): what decides whether a write modified an instance.
    #[must_use]
    pub fn same_content(&self, other: &Instance) -> bool {
        self.value == other.value
            && self.observed_at == other.observed_at
            && self.expires_at == other.expires_at
            && self.evolution_id == other.evolution_id
            && self.proof_seals == other.proof_seals
            && self.sub_attributes == other.sub_attributes
    }
}
