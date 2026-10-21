//! One written attribute instance (clauses 5.5.8, 5.5.12, 5.6.x).
//!
//! Unlike a stored [`Instance`](crate::instance::Instance), a written instance
//! may omit its family and its value, may delete a member with the NGSI-LD
//! Null, and may carry the Temporal Evolution members `instanceId` and
//! `deletedAt` (clauses 4.5.7, 5.6.11, 5.6.12).

use crate::{
    document::{attribute_member::AttributeMember, slot::Slot, written_value::ValueSlot},
    proof_seals::ProofSeals,
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    identifier::Identifier,
    reference::NgsiLdReference,
    time::{system_instant::SystemInstant, written_date_time::DateTimeText},
    time_prop::TimeProp,
};
use cefact_units::UnitCode;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use typed_builder::TypedBuilder;

/// One written attribute instance.
#[derive(Clone, Debug, PartialEq, TypedBuilder)]
pub struct InstanceDocument {
    /// The declared family, when the instance declares one.
    #[builder(default = None)]
    family: Option<AttributeType>,
    /// `datasetId` (clause 4.5.5); never deletable (clauses 5.5.8, 5.5.12).
    #[builder(default = None)]
    dataset_id: Option<Identifier>,
    /// The value, its deletion, or nothing.
    #[builder(default = ValueSlot::Absent)]
    value: ValueSlot,
    /// `observedAt`.
    #[builder(default = None)]
    observed_at: Option<Slot<DateTimeText>>,
    /// `expiresAt` (clause 4.22).
    #[builder(default = None)]
    expires_at: Option<Slot<DateTimeText>>,
    /// `unitCode`.
    #[builder(default = None)]
    unit_code: Option<Slot<UnitCode>>,
    /// `valueType`.
    #[builder(default = None)]
    value_type: Option<Slot<NgsiLdReference>>,
    /// `objectType`.
    #[builder(default = None)]
    object_types: Option<Slot<Vec<NgsiLdReference>>>,
    /// `createdAt`, on documents the broker itself assembled.
    #[builder(default = None)]
    created_at: Option<SystemInstant>,
    /// `modifiedAt`, on documents the broker itself assembled.
    #[builder(default = None)]
    modified_at: Option<SystemInstant>,
    /// `deletedAt` of a Temporal Evolution deletion (clause 4.5.7).
    #[builder(default = None)]
    deleted_at: Option<DateTimeText>,
    /// `instanceId` (clause 4.5.7).
    #[builder(default = None)]
    instance_id: Option<Identifier>,
    /// The data-integrity seals of clause 4.5.2.2.
    #[builder(default = ProofSeals::default())]
    proof_seals: ProofSeals,
    /// The written sub-attributes.
    #[builder(default = IndexMap::new())]
    sub_attributes: IndexMap<AttributeName, AttributeMember>,
}

/// The members of a written instance, given up by
/// [`InstanceDocument::into_parts`] so a conversion moves them rather than
/// copying them.
#[derive(Clone, Debug, PartialEq)]
pub struct InstanceDocumentParts {
    /// The declared family.
    pub family: Option<AttributeType>,
    /// `datasetId`.
    pub dataset_id: Option<Identifier>,
    /// The value slot.
    pub value: ValueSlot,
    /// `observedAt`.
    pub observed_at: Option<Slot<DateTimeText>>,
    /// `expiresAt`.
    pub expires_at: Option<Slot<DateTimeText>>,
    /// `unitCode`.
    pub unit_code: Option<Slot<UnitCode>>,
    /// `valueType`.
    pub value_type: Option<Slot<NgsiLdReference>>,
    /// `objectType`.
    pub object_types: Option<Slot<Vec<NgsiLdReference>>>,
    /// `createdAt`.
    pub created_at: Option<SystemInstant>,
    /// `modifiedAt`.
    pub modified_at: Option<SystemInstant>,
    /// `deletedAt`.
    pub deleted_at: Option<DateTimeText>,
    /// `instanceId`.
    pub instance_id: Option<Identifier>,
    /// The data-integrity seals.
    pub proof_seals: ProofSeals,
    /// The written sub-attributes.
    pub sub_attributes: IndexMap<AttributeName, AttributeMember>,
}

impl InstanceDocument {
    /// The declared family.
    #[must_use]
    pub const fn family(&self) -> Option<AttributeType> {
        self.family
    }

    /// `datasetId`.
    #[must_use]
    pub const fn dataset_id(&self) -> Option<&Identifier> {
        self.dataset_id.as_ref()
    }

    /// The value slot.
    #[must_use]
    pub const fn value(&self) -> &ValueSlot {
        &self.value
    }

    /// `observedAt`.
    #[must_use]
    pub const fn observed_at(&self) -> Option<&Slot<DateTimeText>> {
        self.observed_at.as_ref()
    }

    /// `expiresAt`.
    #[must_use]
    pub const fn expires_at(&self) -> Option<&Slot<DateTimeText>> {
        self.expires_at.as_ref()
    }

    /// `unitCode`.
    #[must_use]
    pub const fn unit_code(&self) -> Option<&Slot<UnitCode>> {
        self.unit_code.as_ref()
    }

    /// `valueType`.
    #[must_use]
    pub const fn value_type(&self) -> Option<&Slot<NgsiLdReference>> {
        self.value_type.as_ref()
    }

    /// `objectType`.
    #[must_use]
    pub const fn object_types(&self) -> Option<&Slot<Vec<NgsiLdReference>>> {
        self.object_types.as_ref()
    }

    /// `createdAt`.
    #[must_use]
    pub const fn created_at(&self) -> Option<SystemInstant> {
        self.created_at
    }

    /// `modifiedAt`.
    #[must_use]
    pub const fn modified_at(&self) -> Option<SystemInstant> {
        self.modified_at
    }

    /// `deletedAt`.
    #[must_use]
    pub const fn deleted_at(&self) -> Option<&DateTimeText> {
        self.deleted_at.as_ref()
    }

    /// `instanceId`.
    #[must_use]
    pub const fn instance_id(&self) -> Option<&Identifier> {
        self.instance_id.as_ref()
    }

    /// The data-integrity seals.
    #[must_use]
    pub const fn proof_seals(&self) -> &ProofSeals {
        &self.proof_seals
    }

    /// The instant the instance carries on one of the four Temporal
    /// Properties (clauses 4.8 and 5.2.21), `None` when it carries none on
    /// that axis: an instance that declared no `observedAt` stands at no
    /// `observedAt` instant, and an `observedAt` the instance removes is none.
    #[must_use]
    pub fn instant_on(&self, timeproperty: TimeProp) -> Option<DateTime<Utc>> {
        match timeproperty {
            TimeProp::Observed => self.observed_at.as_ref().and_then(Slot::as_set).map(DateTimeText::instant),
            TimeProp::Created => self.created_at.map(SystemInstant::get),
            TimeProp::Modified => self.modified_at.map(SystemInstant::get),
            TimeProp::Deleted => self.deleted_at.as_ref().map(DateTimeText::instant),
        }
    }

    /// The written sub-attributes.
    #[must_use]
    pub const fn sub_attributes(&self) -> &IndexMap<AttributeName, AttributeMember> {
        &self.sub_attributes
    }

    /// Replace the value slot.
    pub fn set_value(&mut self, value: ValueSlot) {
        self.value = value;
    }

    /// Replace `observedAt`.
    pub fn set_observed_at(&mut self, observed_at: Option<Slot<DateTimeText>>) {
        self.observed_at = observed_at;
    }

    /// Replace `instanceId`.
    pub fn set_instance_id(&mut self, instance_id: Option<Identifier>) {
        self.instance_id = instance_id;
    }

    /// Drop the output-only members `createdAt`, `modifiedAt`, `deletedAt`
    /// and `instanceId` from this instance, leaving its sub-attributes as they
    /// are.
    ///
    /// The broker generates them (clauses 4.8 and 4.5.7) and ignores any a
    /// producer provides (Table 5.2.5-2: they "shall not be provided by
    /// Context Producers" and "NGSI-LD implementations shall ignore them").
    pub fn forget_output_only_members(&mut self) {
        self.created_at = None;
        self.modified_at = None;
        self.deleted_at = None;
        self.instance_id = None;
    }

    /// The written sub-attributes, for a mutation.
    pub const fn sub_attributes_mut(&mut self) -> &mut IndexMap<AttributeName, AttributeMember> {
        &mut self.sub_attributes
    }

    /// Give the instance up as its members.
    #[must_use]
    pub fn into_parts(self) -> InstanceDocumentParts {
        InstanceDocumentParts {
            family: self.family,
            dataset_id: self.dataset_id,
            value: self.value,
            observed_at: self.observed_at,
            expires_at: self.expires_at,
            unit_code: self.unit_code,
            value_type: self.value_type,
            object_types: self.object_types,
            created_at: self.created_at,
            modified_at: self.modified_at,
            deleted_at: self.deleted_at,
            instance_id: self.instance_id,
            proof_seals: self.proof_seals,
            sub_attributes: self.sub_attributes,
        }
    }
}
