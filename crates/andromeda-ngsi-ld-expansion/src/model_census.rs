//! Counting the values of a write from the typed compact model, before the
//! JSON-LD processor expands it.
//!
//! The typed walk counts as it expands and stops at the first value past the
//! entity value limit, but it leaves a request it does not model to the
//! processor, which counts nothing and builds its whole expanded document
//! first. A decline is not one the client can be denied: any body can carry a
//! context feature the walk declines. So a declined request is counted here,
//! from the model the transport already decoded and under no context, before
//! the processor is handed it (clause 6.3.2).
//!
//! The count never exceeds what the document census counts for the same write
//! under any context: a Property `value` or `valueList` by
//! [`crate::literal_values`], the members whose expansion does not map one to
//! one onto the census by [`crate::typed_walk::member_census`], and nothing for
//! the rest. A request this count refuses is a request the census refuses.

use crate::{
    error::ExpansionError,
    literal_values::count_literal,
    typed_walk::member_census,
    written_member::{WrittenMember, WrittenMemberRef},
};
use andromeda_ngsi_ld_model::{
    attribute::{
        common_fields::CommonAttributeFields,
        instance::{Attribute, AttributeValue},
        merge_instance::MergeInstance,
        merge_member::MergeMember,
        merge_value::MergeValue,
        patched::Patched,
        value::NgsiLdValue,
    },
    entity::{Entity, EntityPatch},
    scope_member::ScopeMember,
};
use andromeda_ngsi_ld_number::json_value::value_census::{JsonValueLimit, ValueTally};
use andromeda_ngsi_ld_representation::payload_budget::PayloadBudgetExceeded;

/// The count of one write's values against the entity value limit.
#[derive(Debug)]
pub(crate) struct ModelCensus {
    /// The values counted so far.
    tally: ValueTally,
}

impl ModelCensus {
    /// A census of nothing yet, under `limit`.
    pub(crate) const fn new(limit: JsonValueLimit) -> ModelCensus {
        ModelCensus { tally: ValueTally::new(limit) }
    }

    /// Count an entity.
    ///
    /// # Errors
    ///
    /// Returns [`PayloadBudgetExceeded::EntityJsonValues`] once the count
    /// passes the limit.
    pub(crate) fn entity<M: WrittenMember>(&self, entity: &Entity<M>) -> Result<(), ExpansionError> {
        self.scope(entity.scope.as_ref())?;
        entity.attribute_members().try_for_each(|(_name, member)| self.member(member))
    }

    /// Count an entity fragment.
    ///
    /// # Errors
    ///
    /// As [`ModelCensus::entity`].
    pub(crate) fn entity_patch<M: WrittenMember>(&self, patch: &EntityPatch<M>) -> Result<(), ExpansionError> {
        self.scope(patch.scope.as_ref())?;
        patch.attribute_members().try_for_each(|(_name, member)| self.member(member))
    }

    /// Count the one attribute member of an attribute fragment.
    ///
    /// # Errors
    ///
    /// As [`ModelCensus::entity`].
    pub(crate) fn attribute<M: WrittenMember>(&self, member: &M) -> Result<(), ExpansionError> {
        self.member(member)
    }

    /// Count `values` values a member holds by the census rule for it.
    fn admit(&self, values: usize) -> Result<(), ExpansionError> {
        self.tally
            .admit(values)
            .map_err(|exceeded| ExpansionError::PayloadBudget(PayloadBudgetExceeded::EntityJsonValues { limit: exceeded.limit }))
    }

    /// Count the entity `scope`.
    fn scope(&self, member: Option<&ScopeMember>) -> Result<(), ExpansionError> {
        member.map_or(Ok(()), |member| self.admit(member_census::scope(member)))
    }

    /// Count the JSON a Property carries as its value.
    fn literal(&self, value: &NgsiLdValue) -> Result<(), ExpansionError> {
        count_literal(&self.tally, value.literal().piece().text())
    }

    /// Count one attribute member of an entity or of a merge fragment.
    fn member<M: WrittenMember>(&self, member: &M) -> Result<(), ExpansionError> {
        match member.written() {
            WrittenMemberRef::Attribute(member) => self.attribute_member(member),
            WrittenMemberRef::Merge(member) => self.merge_member(member),
        }
    }

    /// Count the instances of an entity's attribute member.
    fn attribute_member(&self, member: &AttributeValue) -> Result<(), ExpansionError> {
        match member {
            AttributeValue::Single(instance) => self.instance(instance),
            AttributeValue::Multiple(instances) => instances.iter().try_for_each(|instance| self.instance(instance)),
            AttributeValue::NgsiLdNull => Ok(()),
        }
    }

    /// Count one attribute instance: its own family's value, then its
    /// sub-attributes.
    fn instance(&self, instance: &Attribute) -> Result<(), ExpansionError> {
        match instance {
            Attribute::Property(property) => self.literal(&property.value),
            Attribute::Relationship(relationship) => self.admit(member_census::listed(&relationship.object)),
            Attribute::VocabProperty(vocab_property) => self.admit(member_census::listed(&vocab_property.vocab)),
            Attribute::LanguageProperty(language_property) => self.admit(member_census::language_map(&language_property.language_map)),
            Attribute::ListProperty(list_property) => list_property.value_list.iter().try_for_each(|value| self.literal(value)),
            Attribute::ListRelationship(list_relationship) => self.admit(member_census::object_list(&list_relationship.object_list)),
            Attribute::GeoProperty(_) | Attribute::JsonProperty(_) => Ok(()),
        }?;
        self.sub_attributes(instance.common())
    }

    /// Count the sub-attributes of an instance.
    fn sub_attributes(&self, common: &CommonAttributeFields) -> Result<(), ExpansionError> {
        common.sub_attributes.values().try_for_each(|member| self.attribute_member(member))
    }

    /// Count the instances of a merge fragment's attribute member.
    fn merge_member(&self, member: &MergeMember) -> Result<(), ExpansionError> {
        match member {
            MergeMember::Single(instance) => self.merge_instance(instance),
            MergeMember::Multiple(instances) => instances.iter().try_for_each(|instance| self.merge_instance(instance)),
            MergeMember::NgsiLdNull => Ok(()),
        }
    }

    /// Count one merge instance: the value it sets, then its sub-attributes.
    fn merge_instance(&self, instance: &MergeInstance) -> Result<(), ExpansionError> {
        match &instance.value {
            Some(Patched::Set(value)) => self.merge_value(value),
            Some(Patched::Null) | None => Ok(()),
        }?;
        instance.sub_attributes.values().try_for_each(|member| self.merge_member(member))
    }

    /// Count the value a merge instance sets.
    fn merge_value(&self, value: &MergeValue) -> Result<(), ExpansionError> {
        match value {
            MergeValue::Property(value) => self.literal(value),
            MergeValue::Relationship(object) => self.admit(member_census::listed(object)),
            MergeValue::VocabProperty(vocab) => self.admit(member_census::listed(vocab)),
            MergeValue::LanguageProperty(language_map) => self.admit(member_census::language_map_patch(language_map)),
            MergeValue::ListProperty(values) => values.iter().try_for_each(|value| self.literal(value)),
            MergeValue::ListRelationship(object_list) => self.admit(member_census::object_list(object_list)),
            MergeValue::GeoProperty(_) | MergeValue::JsonProperty(_) => Ok(()),
        }
    }
}
