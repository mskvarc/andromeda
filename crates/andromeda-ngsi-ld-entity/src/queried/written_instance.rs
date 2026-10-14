//! One written attribute instance as the `q` evaluator reads it.
//!
//! A Temporal Evolution fragment (clauses 5.6.11, 5.6.12) records instances as
//! the client wrote them, and a filter over that record reads them as written:
//! a member the instance sets is held, a member it deletes or omits is not, and
//! a deletion of the whole instance (`deletedAt`, or its family's deletion
//! encoding) holds no value.

use crate::{
    date_time_text::DateTimeText,
    document::{
        attribute_member::AttributeMember,
        instance_document::InstanceDocument,
        slot::Slot,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::ValueNode,
    queried::family_values::{FamilyValues, language_strings},
    value_mode::Fragment,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier, reference::NgsiLdReference};
use andromeda_ngsi_ld_query::queried_entity::{attribute_reading::QueriedInstance, instance_metadata::InstanceMetadata, value_item::ValueItem};
use cefact_units::UnitCode;
use std::borrow::Cow;

impl<'a> QueriedInstance<'a> for &'a InstanceDocument {
    type Node = &'a ValueNode<Fragment>;

    /// The family the instance declares, else the one its value member implies
    /// (clause 4.5.2: a typeless `value` is a Property).
    fn family(self) -> AttributeType {
        InstanceDocument::family(self).unwrap_or_else(|| match self.value() {
            ValueSlot::Set(value) => value.carrier().implied_family(),
            ValueSlot::Remove(carrier) => carrier.implied_family(),
            ValueSlot::Absent => AttributeType::Property,
        })
    }

    fn values(self) -> impl Iterator<Item = ValueItem<'a, &'a ValueNode<Fragment>>> {
        match self.value() {
            ValueSlot::Set(value) => FamilyValues::of_written(value),
            ValueSlot::Absent | ValueSlot::Remove(_) => FamilyValues::none(),
        }
    }

    fn language_values(self, language: &str) -> impl Iterator<Item = ValueItem<'a, &'a ValueNode<Fragment>>> {
        let entries = match self.value() {
            ValueSlot::Set(WrittenValue::LanguageMap(entries)) => entries.entries(),
            ValueSlot::Set(
                WrittenValue::Value(_)
                | WrittenValue::Geometry(_)
                | WrittenValue::Object(_)
                | WrittenValue::Vocab(_)
                | WrittenValue::Json(_)
                | WrittenValue::ValueList(_)
                | WrittenValue::ObjectList(_),
            )
            | ValueSlot::Absent
            | ValueSlot::Remove(_) => &[],
        };
        language_strings(entries, language)
    }

    fn sub_attribute_instances(self, name: &AttributeName) -> impl Iterator<Item = &'a InstanceDocument> {
        self.sub_attributes().get(name.as_str()).into_iter().flat_map(AttributeMember::instances)
    }

    fn metadata(self, member: InstanceMetadata) -> Option<Cow<'a, str>> {
        match member {
            InstanceMetadata::ObservedAt => set(self.observed_at()).map(DateTimeText::as_str).map(Cow::Borrowed),
            InstanceMetadata::CreatedAt => self.created_at().map(|instant| Cow::Owned(instant.to_string())),
            InstanceMetadata::ModifiedAt => self.modified_at().map(|instant| Cow::Owned(instant.to_string())),
            InstanceMetadata::ExpiresAt => set(self.expires_at()).map(DateTimeText::as_str).map(Cow::Borrowed),
            InstanceMetadata::DatasetId => self.dataset_id().map(Identifier::as_str).map(Cow::Borrowed),
            InstanceMetadata::InstanceId => self.instance_id().map(Identifier::as_str).map(Cow::Borrowed),
            InstanceMetadata::UnitCode => set(self.unit_code()).copied().map(UnitCode::as_str).map(Cow::Borrowed),
            InstanceMetadata::ValueType => set(self.value_type()).map(NgsiLdReference::as_str).map(Cow::Borrowed),
            InstanceMetadata::ObjectType => set(self.object_types())
                .and_then(|object_types| object_types.first())
                .map(NgsiLdReference::as_str)
                .map(Cow::Borrowed),
        }
    }
}

/// The value of a member the instance sets; nothing for one it deletes or
/// omits.
fn set<T>(slot: Option<&Slot<T>>) -> Option<&T> {
    slot.and_then(Slot::as_set)
}
