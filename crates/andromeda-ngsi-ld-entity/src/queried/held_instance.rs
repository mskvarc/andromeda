//! One attribute instance a state holds, as the `q` evaluator reads it.

use crate::{
    attribute_instances::AttributeInstances,
    expanded_value::ValueNode,
    family_value::FamilyValue,
    instance::Instance,
    queried::family_values::{FamilyValues, language_strings},
    value_mode::Stored,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier, reference::NgsiLdReference};
use andromeda_ngsi_ld_query::{
    filter::{language_matching::LanguageMatching, target_language::LanguageSelector},
    queried_entity::{attribute_reading::QueriedInstance, instance_metadata::InstanceMetadata, value_item::ValueItem},
};
use std::borrow::Cow;

/// One attribute instance of a state, with the `datasetId` it is held under.
///
/// A state keys an attribute's instances by `datasetId` (clause 4.5.5), so the
/// instance itself does not carry it; clause 4.9 nevertheless lets a path
/// address it (`speed.datasetId`), which is why the handle pairs the two.
#[derive(Clone, Copy, Debug)]
pub struct HeldInstance<'a> {
    /// The `datasetId`; `None` for the default instance.
    dataset_id: Option<&'a Identifier>,
    /// The instance.
    instance: &'a Instance,
}

impl<'a> HeldInstance<'a> {
    /// The instance held under `dataset_id`.
    #[must_use]
    pub const fn new(dataset_id: Option<&'a Identifier>, instance: &'a Instance) -> HeldInstance<'a> {
        HeldInstance { dataset_id, instance }
    }

    /// The `datasetId` the instance is held under.
    #[must_use]
    pub const fn dataset_id(self) -> Option<&'a Identifier> {
        self.dataset_id
    }

    /// The instance.
    #[must_use]
    pub const fn instance(self) -> &'a Instance {
        self.instance
    }

    /// Every instance of one attribute, in stored order.
    pub fn every(instances: &'a AttributeInstances) -> impl Iterator<Item = HeldInstance<'a>> {
        instances.iter().map(|(dataset_id, instance)| HeldInstance::new(dataset_id, instance))
    }
}

impl<'a> QueriedInstance<'a> for HeldInstance<'a> {
    type Node = &'a ValueNode<Stored>;

    fn family(self) -> AttributeType {
        self.instance.family()
    }

    fn values(self) -> impl Iterator<Item = ValueItem<'a, &'a ValueNode<Stored>>> {
        FamilyValues::of_state(self.instance.value())
    }

    fn language_values(self, wanted: LanguageSelector<'_>, matching: LanguageMatching) -> impl Iterator<Item = ValueItem<'a, &'a ValueNode<Stored>>> {
        let entries = match self.instance.value() {
            FamilyValue::LanguageProperty { entries, .. } => entries.entries(),
            FamilyValue::Property { .. }
            | FamilyValue::GeoProperty { .. }
            | FamilyValue::Relationship { .. }
            | FamilyValue::VocabProperty { .. }
            | FamilyValue::JsonProperty { .. }
            | FamilyValue::ListProperty { .. }
            | FamilyValue::ListRelationship { .. } => &[],
        };
        language_strings(entries, wanted, matching)
    }

    fn sub_attribute_instances(self, name: &AttributeName) -> impl Iterator<Item = HeldInstance<'a>> {
        self.instance.sub_attributes().get(name.as_str()).into_iter().flat_map(HeldInstance::every)
    }

    fn metadata(self, member: InstanceMetadata) -> Option<Cow<'a, str>> {
        let instance = self.instance;
        match member {
            InstanceMetadata::ObservedAt => instance.observed_at().map(|instant| Cow::Owned(instant.to_string())),
            InstanceMetadata::CreatedAt => instance.created_at().map(|instant| Cow::Owned(instant.to_string())),
            InstanceMetadata::ModifiedAt => instance.modified_at().map(|instant| Cow::Owned(instant.to_string())),
            InstanceMetadata::ExpiresAt => instance.expires_at().map(|instant| Cow::Owned(instant.to_string())),
            InstanceMetadata::DatasetId => self.dataset_id.map(Identifier::as_str).map(Cow::Borrowed),
            // Table 5.2.5-2 has `instanceId` only in the temporal representation,
            // so a held instance never carries one.
            InstanceMetadata::InstanceId => None,
            InstanceMetadata::UnitCode => instance.value().unit_code().map(|unit_code| Cow::Borrowed(unit_code.as_str())),
            InstanceMetadata::ValueType => instance.value().value_type().map(NgsiLdReference::as_str).map(Cow::Borrowed),
            InstanceMetadata::ObjectType => instance.value().object_types().first().map(NgsiLdReference::as_str).map(Cow::Borrowed),
        }
    }
}
