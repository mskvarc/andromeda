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
//! written, and decides how each deletion the fragment carries is written.

use crate::{
    entity_body::instance_fallback::kept_instance,
    version::{
        adapted_payload::AdaptedPayload,
        deletion::{DeletionEncoding, DeletionSite},
        fallback::{InstanceMember, VersionFallback},
        list_as_relationship::ListAsRelationship,
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, spec_version::SpecVersion, time::rendering::instant_text};
use andromeda_ngsi_ld_model::attribute::{
    merge_instance::MergeInstance,
    merge_member::MergeMember,
    merge_value::MergeValue,
    nested_null::NullProperty,
    patched::Patched,
};
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, keyword, ngsi_ld_null::NULL_URI};
use serde::{Serialize, Serializer, ser::SerializeMap};

/// One member of a forwarded Attribute Fragment.
#[derive(Debug, Clone, Copy)]
pub struct ForwardedFragment<'a> {
    /// The merge member written.
    member: &'a MergeMember,
    /// The version the source conforms to, when the registration names one.
    conformance: Option<SpecVersion>,
    /// The clause 4.3.6.8 fallback the member is written under, when the
    /// source conforms to an older specification version.
    fallback: Option<VersionFallback>,
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
        ForwardedFragment { member, conformance, fallback }
    }

    /// `member`, a sub-attribute of an instance of this member.
    const fn sub_attribute(self, member: &'a MergeMember) -> ForwardedFragment<'a> {
        ForwardedFragment { member, ..self }
    }

    /// `instance`, one instance of the member.
    const fn instance(self, instance: &'a MergeInstance) -> FragmentInstance<'a> {
        FragmentInstance { instance, member: self }
    }

    /// How the source reads a deletion at `site`, if it reads one there.
    const fn deletion(self, site: DeletionSite) -> Option<DeletionEncoding> {
        DeletionEncoding::for_target(self.conformance, site)
    }
}

impl Serialize for ForwardedFragment<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let instances = match self.member {
            MergeMember::NgsiLdNull => {
                return match self.deletion(DeletionSite::Attribute) {
                    Some(DeletionEncoding::NgsiLdNull) => serializer.serialize_str(NULL_URI.as_str()),
                    Some(DeletionEncoding::JsonNull) => serializer.serialize_unit(),
                    // A source that cannot delete the attribute is sent the
                    // empty fragment, which clause 5.5.8 merges as no change.
                    None => serializer.serialize_map(Some(0))?.end(),
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
        let merged = self.member.deletion(DeletionSite::MergedMember);
        let mut map = serializer.serialize_map(None)?;
        if let Some(family) = self.written_family() {
            map.serialize_entry(keyword::TYPE_ALIAS, family.short_name())?;
            match &instance.value {
                Some(Patched::Set(value)) => map.serialize_entry(family.compact_value_key(), &WrittenValue::of(value, family))?,
                Some(Patched::Null) => deletion_entry(
                    &mut map,
                    family.compact_value_key(),
                    self.member.deletion(DeletionSite::Attribute),
                    NULL_URI.as_str(),
                )?,
                None => {}
            }
        }
        let kept = |member| self.keeps(member);
        patched_entry(
            &mut map,
            compact_properties::UNIT_CODE,
            instance.unit_code.as_ref().filter(|_| kept(InstanceMember::UnitCode)),
            merged,
        )?;
        patched_entry(
            &mut map,
            compact_properties::VALUE_TYPE,
            instance.value_type.as_ref().filter(|_| kept(InstanceMember::ValueType)),
            merged,
        )?;
        patched_entry(
            &mut map,
            compact_properties::OBJECT_TYPE,
            instance.object_type.as_ref().filter(|_| kept(InstanceMember::ObjectType)),
            merged,
        )?;
        optional_entry(&mut map, compact_properties::ENTITY_ID_SEALED, instance.entity_id_sealed.as_ref())?;
        optional_entry(&mut map, compact_properties::ENTITY_TYPE_SEALED, instance.entity_type_sealed.as_ref())?;
        optional_entry(
            &mut map,
            compact_properties::DATASET_ID,
            instance.dataset_id.as_ref().filter(|_| kept(InstanceMember::DatasetId)),
        )?;
        patched_entry(
            &mut map,
            compact_properties::EXPIRES_AT,
            instance
                .expires_at
                .as_ref()
                .filter(|_| kept(InstanceMember::ExpiresAt))
                .map(|patched| patched.map(|at| instant_text(*at)))
                .as_ref(),
            merged,
        )?;
        optional_entry(&mut map, compact_properties::INSTANCE_ID, instance.instance_id.as_ref())?;
        patched_entry(
            &mut map,
            compact_properties::OBSERVED_AT,
            instance
                .observed_at
                .as_ref()
                .filter(|_| kept(InstanceMember::ObservedAt))
                .map(|patched| patched.map(|at| instant_text(*at)))
                .as_ref(),
            merged,
        )?;
        for (name, member) in &instance.sub_attributes {
            match member {
                // Clause 4.5.2.3: the Null removing a sub-attribute travels as
                // the Property whose value it is.
                MergeMember::NgsiLdNull => deletion_entry(&mut map, name.as_str(), merged, &NullProperty)?,
                MergeMember::Single(_) | MergeMember::Multiple(_) => map.serialize_entry(name.as_str(), &self.member.sub_attribute(member))?,
            }
        }
        map.end()
    }
}

/// A fragment's value member as the family it is written as holds it.
#[derive(Debug, Clone, Copy)]
enum WrittenValue<'a> {
    /// The value as the fragment wrote it.
    Held(&'a MergeValue),
    /// A `ListRelationship`'s targets, written as the `object` of the
    /// Relationship Table 4.3.6.8-1 reformats it as.
    ListAsRelationship(ListAsRelationship<'a>),
}

impl<'a> WrittenValue<'a> {
    /// `value`, written under the value key of `family`.
    fn of(value: &'a MergeValue, family: AttributeType) -> WrittenValue<'a> {
        match value {
            MergeValue::ListRelationship(list) if family == AttributeType::Relationship => WrittenValue::ListAsRelationship(ListAsRelationship::new(&list.0)),
            MergeValue::Property(_)
            | MergeValue::Relationship(_)
            | MergeValue::GeoProperty(_)
            | MergeValue::VocabProperty(_)
            | MergeValue::LanguageProperty(_)
            | MergeValue::JsonProperty(_)
            | MergeValue::ListProperty(_)
            | MergeValue::ListRelationship(_) => WrittenValue::Held(value),
        }
    }
}

impl Serialize for WrittenValue<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            WrittenValue::Held(value) => value.serialize(serializer),
            WrittenValue::ListAsRelationship(object) => object.serialize(serializer),
        }
    }
}

/// Write `value` under `key` when present.
fn optional_entry<M: SerializeMap, T: Serialize>(map: &mut M, key: &str, value: Option<&T>) -> Result<(), M::Error> {
    value.map_or(Ok(()), |value| map.serialize_entry(key, value))
}

/// Write the member `key` when present: its value, or its deletion as
/// `encoding` writes one.
fn patched_entry<M: SerializeMap, T: Serialize>(
    map: &mut M,
    key: &str,
    value: Option<&Patched<T>>,
    encoding: Option<DeletionEncoding>,
) -> Result<(), M::Error> {
    match value {
        Some(Patched::Set(value)) => map.serialize_entry(key, value),
        Some(Patched::Null) => deletion_entry(map, key, encoding, NULL_URI.as_str()),
        None => Ok(()),
    }
}

/// Write the deletion of the member `key` as `encoding` says, `ngsi_ld_null`
/// being the NGSI-LD Null that deletes it; a deletion the source cannot read
/// is left out.
fn deletion_entry<M: SerializeMap, N: Serialize + ?Sized>(
    map: &mut M,
    key: &str,
    encoding: Option<DeletionEncoding>,
    ngsi_ld_null: &N,
) -> Result<(), M::Error> {
    match encoding {
        Some(DeletionEncoding::NgsiLdNull) => map.serialize_entry(key, ngsi_ld_null),
        Some(DeletionEncoding::JsonNull) => map.serialize_entry(key, &()),
        None => Ok(()),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
