//! One attribute instance (clauses 4.5.1, 4.5.5).
//!
//! The instance's identity, its `datasetId`, is the key it is held under in
//! [`AttributeInstances`](crate::attribute_instances::AttributeInstances), not
//! a member of the instance itself: two instances with one identity cannot be
//! held at all. An instance holds no `instanceId`: Table 5.2.5-2 has it "Only
//! used in temporal representation of Properties", and clause 5.2.5 has a Core
//! write ignore one a producer supplies.

use crate::{attributes::Attributes, family_value::FamilyValue, proof_seals::ProofSeals};
use andromeda_ngsi_ld_lexicon::{
    attribute_type::AttributeType,
    time::{date_time::NgsiLdDateTime, system_instant::SystemInstant},
};
use typed_builder::TypedBuilder;

/// One attribute instance: its family and value, its temporal members and its
/// sub-attributes.
#[derive(Clone, Debug, PartialEq, TypedBuilder)]
pub struct Instance {
    /// The family and its value.
    value: FamilyValue,
    /// `observedAt` (clause 4.5.1).
    #[builder(default = None)]
    observed_at: Option<NgsiLdDateTime>,
    /// `expiresAt` (clause 4.22).
    #[builder(default = None)]
    expires_at: Option<NgsiLdDateTime>,
    /// `createdAt` (clause 4.8), stamped by the broker.
    #[builder(default = None)]
    created_at: Option<SystemInstant>,
    /// `modifiedAt` (clause 4.8), stamped by the broker.
    #[builder(default = None)]
    modified_at: Option<SystemInstant>,
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
    pub const fn observed_at(&self) -> Option<NgsiLdDateTime> {
        self.observed_at
    }

    /// `expiresAt`, when present.
    #[must_use]
    pub const fn expires_at(&self) -> Option<NgsiLdDateTime> {
        self.expires_at
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
    pub const fn set_observed_at(&mut self, observed_at: Option<NgsiLdDateTime>) {
        self.observed_at = observed_at;
    }

    /// Replace `expiresAt`.
    pub const fn set_expires_at(&mut self, expires_at: Option<NgsiLdDateTime>) {
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

    /// The sub-attributes, for a mutation.
    pub const fn sub_attributes_mut(&mut self) -> &mut Attributes {
        &mut self.sub_attributes
    }

    /// Whether two instances agree on everything but their system times
    /// (clause 4.8), at every depth: what decides whether a write modified an
    /// instance.
    ///
    /// Clause 5.8.6 has an attribute change "when any of the members
    /// (including children) in its corresponding JSON-LD node is updated with
    /// a value different than the existing one". The attribute-level
    /// `modifiedAt`, the notification change, the Temporal Evolution
    /// observation of a Core write and the current-state fold of a Temporal
    /// API write all read a modification off this one test.
    #[must_use]
    pub fn same_content(&self, other: &Instance) -> bool {
        self.value == other.value
            && self.observed_at == other.observed_at
            && self.expires_at == other.expires_at
            && self.proof_seals == other.proof_seals
            && self.sub_attributes.same_content(&other.sub_attributes)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_instances::AttributeInstances,
        instance::Instance,
        operation::speed_entity::{date_time, name, number, property},
    };
    use andromeda_ngsi_ld_lexicon::time::system_instant::SystemInstant;

    const ACCURACY: &str = "https://example.org/accuracy";

    /// A Property holding `value`, observed at `observed_at`, carrying an
    /// `accuracy` sub-Property holding `accuracy`.
    fn reading(value: i64, observed_at: &str, accuracy: i64) -> Instance {
        let mut instance = property(number(value));
        instance.set_observed_at(Some(date_time(observed_at)));
        instance
            .sub_attributes_mut()
            .insert(name(ACCURACY), AttributeInstances::one(None, property(number(accuracy))));
        instance
    }

    #[test]
    fn an_equal_value_with_equal_members_is_the_same_content() {
        assert!(reading(50, "2026-10-19T10:00:00Z", 1).same_content(&reading(50, "2026-10-19T10:00:00Z", 1)));
    }

    #[test]
    fn a_changed_value_is_different_content() {
        assert!(!reading(51, "2026-10-19T10:00:00Z", 1).same_content(&reading(50, "2026-10-19T10:00:00Z", 1)));
    }

    #[test]
    fn a_changed_sub_attribute_is_different_content() {
        assert!(!reading(50, "2026-10-19T10:00:00Z", 2).same_content(&reading(50, "2026-10-19T10:00:00Z", 1)));
    }

    #[test]
    fn a_changed_observed_at_is_different_content() {
        assert!(!reading(50, "2026-10-19T10:01:00Z", 1).same_content(&reading(50, "2026-10-19T10:00:00Z", 1)));
    }

    #[test]
    fn a_removed_observed_at_is_different_content() {
        let mut unobserved = reading(50, "2026-10-19T10:00:00Z", 1);
        unobserved.set_observed_at(None);
        assert!(!unobserved.same_content(&reading(50, "2026-10-19T10:00:00Z", 1)));
    }

    /// Clause 4.8: `createdAt` and `modifiedAt` are stamped by the broker on
    /// every write that touches an instance, so they are not content.
    #[test]
    fn differing_system_times_are_the_same_content() {
        let stamp = |at: &str| -> SystemInstant { at.parse().expect("a valid instant") };
        let mut earlier = reading(50, "2026-10-19T10:00:00Z", 1);
        earlier.set_created_at(Some(stamp("2026-10-19T10:00:00.000000Z")));
        earlier.set_modified_at(Some(stamp("2026-10-19T10:00:00.000000Z")));
        let mut later = reading(50, "2026-10-19T10:00:00Z", 1);
        later.set_created_at(Some(stamp("2026-10-19T10:00:00.000000Z")));
        later.set_modified_at(Some(stamp("2026-10-19T11:00:00.000000Z")));
        assert!(earlier.same_content(&later));
    }
}
