//! The Attribute Fragment of a Partial Attribute Update forwarded to a context
//! source (clauses 4.3.6.8, 5.5.12 and 5.6.4), written from its typed form.
//!
//! The fragment is a merge member: it changes only the members it names, so an
//! instance may leave out its `type` and its value member, and the source
//! receives it so. Read as a whole Attribute, an instance naming neither would
//! be the bare value of a concise Property (clause 4.5.2.3), which is not what
//! the client wrote. An instance that names or implies its family is written
//! with it in `type` (clause 4.5.1). The registration's `ngsildConformance` is
//! the clause 4.3.6.8 fallback applied member by member as the fragment is
//! written.

use crate::{
    entity_body::instance_fallback::kept_instance,
    version::{
        adapted_payload::AdaptedPayload,
        fallback::{InstanceMember, VersionFallback},
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, spec_version::SpecVersion, time::rendering::instant_text, uri::Uri};
use andromeda_ngsi_ld_model::attribute::{
    merge_instance::MergeInstance,
    merge_member::MergeMember,
    merge_value::MergeValue,
    nested_null::NullProperty,
    patched::Patched,
};
use andromeda_ngsi_ld_vocabulary::{NULL_URI, compact::properties as compact_properties};
use serde::{Serialize, Serializer, ser::SerializeMap};

/// The member an instance names its family in.
const TYPE_MEMBER: &str = "type";

/// Where a fragment member stands, which decides how its NGSI-LD Null is
/// written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemberStanding {
    /// The attribute the request path names: the bare Null.
    Attribute,
    /// A sub-attribute: the Property whose value is the Null (clause 4.5.2.3),
    /// which clause 5.5.12 reads as the removal of the sub-attribute.
    SubAttribute,
}

/// One member of a forwarded Attribute Fragment.
#[derive(Debug, Clone, Copy)]
pub struct ForwardedFragment<'a> {
    /// The merge member written.
    member: &'a MergeMember,
    /// The clause 4.3.6.8 fallback the member is written under, when the
    /// source conforms to an older specification version.
    fallback: Option<VersionFallback>,
    /// Where the member stands.
    standing: MemberStanding,
}

impl<'a> ForwardedFragment<'a> {
    /// `member`, the fragment of the attribute a request path names, written
    /// for a source that conforms to `conformance`, when the registration names
    /// a version.
    #[must_use]
    pub const fn new(member: &'a MergeMember, conformance: Option<SpecVersion>) -> ForwardedFragment<'a> {
        let fallback = match conformance {
            Some(version) => VersionFallback::new(version, AdaptedPayload::Entity),
            None => None,
        };
        ForwardedFragment {
            member,
            fallback,
            standing: MemberStanding::Attribute,
        }
    }

    /// `member`, a sub-attribute of an instance of this member.
    const fn sub_attribute(self, member: &'a MergeMember) -> ForwardedFragment<'a> {
        ForwardedFragment {
            member,
            fallback: self.fallback,
            standing: MemberStanding::SubAttribute,
        }
    }

    /// `instance`, one instance of the member.
    const fn instance(self, instance: &'a MergeInstance) -> FragmentInstance<'a> {
        FragmentInstance { instance, member: self }
    }
}

impl Serialize for ForwardedFragment<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let instances = match self.member {
            MergeMember::NgsiLdNull => {
                return match self.standing {
                    MemberStanding::Attribute => serializer.serialize_str(NULL_URI.as_str()),
                    MemberStanding::SubAttribute => NullProperty.serialize(serializer),
                };
            }
            MergeMember::Single(instance) => return self.instance(instance).serialize(serializer),
            MergeMember::Multiple(instances) => instances,
        };
        // Table 4.3.6.8-1 notes 2 and 3: a target below 1.3 receives one
        // instance, the default one by preference.
        let kept = self
            .fallback
            .filter(|fallback| fallback.collapses_instances())
            .and_then(|_| kept_instance(instances, |instance| instance.dataset_id.as_ref()));
        match kept {
            Some(kept) => self.instance(kept).serialize(serializer),
            None => serializer.collect_seq(instances.iter().map(|instance| self.instance(instance))),
        }
    }
}

/// One instance of a forwarded fragment member.
#[derive(Debug, Clone, Copy)]
struct FragmentInstance<'a> {
    /// The instance written.
    instance: &'a MergeInstance,
    /// The member it is an instance of.
    member: ForwardedFragment<'a>,
}

impl FragmentInstance<'_> {
    /// The family the instance is written as, when it names or implies one:
    /// its own, or the one Table 4.3.6.8-1 generalizes it to.
    fn written_family(&self) -> Option<AttributeType> {
        let family = self.instance.family?;
        Some(self.member.fallback.map_or(family, |fallback| fallback.family(family)))
    }

    /// Whether the instance keeps `member` under the fallback (Tables
    /// 4.3.6.8-2 and -3). An instance naming no family is read as the Property
    /// clause 4.5.2.3 infers for it.
    fn keeps(&self, member: InstanceMember) -> bool {
        let family = self.written_family().unwrap_or(AttributeType::Property);
        self.member.fallback.is_none_or(|fallback| fallback.keeps_instance_member(family, member))
    }
}

impl Serialize for FragmentInstance<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let instance = self.instance;
        let mut map = serializer.serialize_map(None)?;
        if let Some(family) = self.written_family() {
            map.serialize_entry(TYPE_MEMBER, family.short_name())?;
            if let Some(value) = &instance.value {
                map.serialize_entry(family.compact_value_key(), &WrittenValue::of(value, family))?;
            }
        }
        let kept = |member| self.keeps(member);
        optional_entry(
            &mut map,
            compact_properties::UNIT_CODE,
            instance.unit_code.as_ref().filter(|_| kept(InstanceMember::UnitCode)),
        )?;
        optional_entry(
            &mut map,
            compact_properties::VALUE_TYPE,
            instance.value_type.as_ref().filter(|_| kept(InstanceMember::ValueType)),
        )?;
        optional_entry(
            &mut map,
            compact_properties::OBJECT_TYPE,
            instance.object_type.as_ref().filter(|_| kept(InstanceMember::ObjectType)),
        )?;
        optional_entry(&mut map, compact_properties::ENTITY_ID_SEALED, instance.entity_id_sealed.as_ref())?;
        optional_entry(&mut map, compact_properties::ENTITY_TYPE_SEALED, instance.entity_type_sealed.as_ref())?;
        optional_entry(
            &mut map,
            compact_properties::DATASET_ID,
            instance.dataset_id.as_ref().filter(|_| kept(InstanceMember::DatasetId)),
        )?;
        optional_entry(
            &mut map,
            compact_properties::EXPIRES_AT,
            instance
                .expires_at
                .as_ref()
                .filter(|_| kept(InstanceMember::ExpiresAt))
                .map(|patched| patched.map(|at| instant_text(*at)))
                .as_ref(),
        )?;
        optional_entry(&mut map, compact_properties::INSTANCE_ID, instance.instance_id.as_ref())?;
        optional_entry(
            &mut map,
            compact_properties::OBSERVED_AT,
            instance
                .observed_at
                .as_ref()
                .filter(|_| kept(InstanceMember::ObservedAt))
                .map(|patched| patched.map(|at| instant_text(*at)))
                .as_ref(),
        )?;
        for (name, member) in &instance.sub_attributes {
            map.serialize_entry(name.as_str(), &self.member.sub_attribute(member))?;
        }
        map.end()
    }
}

/// A fragment's value member as the family it is written as holds it.
#[derive(Debug, Clone, Copy)]
enum WrittenValue<'a> {
    /// The value as the fragment wrote it, or its removal.
    Held(&'a Patched<MergeValue>),
    /// The one target a `ListRelationship` written as a Relationship keeps
    /// (Table 4.3.6.8-1): the first of its list.
    FirstTarget(Option<&'a Uri>),
}

impl<'a> WrittenValue<'a> {
    /// `value`, written under the value key of `family`.
    fn of(value: &'a Patched<MergeValue>, family: AttributeType) -> WrittenValue<'a> {
        match value {
            Patched::Set(MergeValue::ListRelationship(list)) if family == AttributeType::Relationship => WrittenValue::FirstTarget(list.0.first()),
            Patched::Set(_) | Patched::Null => WrittenValue::Held(value),
        }
    }
}

impl Serialize for WrittenValue<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            WrittenValue::Held(value) => value.serialize(serializer),
            WrittenValue::FirstTarget(target) => target.serialize(serializer),
        }
    }
}

/// Write `value` under `key` when present.
fn optional_entry<M: SerializeMap, T: Serialize>(map: &mut M, key: &str, value: Option<&T>) -> Result<(), M::Error> {
    value.map_or(Ok(()), |value| map.serialize_entry(key, value))
}

#[cfg(test)]
mod tests;
