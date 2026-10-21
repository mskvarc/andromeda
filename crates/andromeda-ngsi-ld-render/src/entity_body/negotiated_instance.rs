//! The `Property` a clause 4.15 `lang` filter answers a top-level
//! `LanguageProperty` as, in a reified representation.

use crate::{
    entity_body::{
        common_members::{serialize_present, serialize_sub_attributes},
        member_walk::MemberWalk,
        reified_member::Reification,
    },
    language_negotiation::NegotiatedEntry,
    version::fallback::InstanceMember,
};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, time::rendering::instant_text};
use andromeda_ngsi_ld_model::attribute::common_fields::CommonAttributeFields;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde::{Serialize, Serializer, ser::SerializeMap};

/// The `Property` a negotiated `LanguageProperty` is answered as (clause 4.15):
/// the selected entry as its value, the tag as `lang`, and of everything else
/// the attribute carries only its `observedAt`, its `datasetId` and its
/// sub-attributes, as far as the version fallback keeps a Property's.
pub(crate) struct NegotiatedInstance<'a> {
    /// The entry negotiation selected.
    pub(crate) entry: NegotiatedEntry<'a>,
    /// The members the language property carries beside its map.
    pub(crate) common: &'a CommonAttributeFields,
    /// The representation it is written in.
    pub(crate) reification: Reification,
    /// The walk its sub-attributes are written under.
    pub(crate) walk: MemberWalk<'a>,
}

impl Serialize for NegotiatedInstance<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let keeps = |member| self.walk.keeps_instance_member(AttributeType::Property, member);
        let mut object = serializer.serialize_map(None)?;
        if self.reification == Reification::Normalized {
            object.serialize_entry("type", &AttributeType::Property)?;
        }
        object.serialize_entry(compact_properties::VALUE, self.entry.value)?;
        object.serialize_entry(compact_properties::LANG, &self.entry.lang)?;
        serialize_present(
            &mut object,
            compact_properties::OBSERVED_AT,
            self.common.observed_at.filter(|_| keeps(InstanceMember::ObservedAt)).map(instant_text),
        )?;
        serialize_present(
            &mut object,
            compact_properties::DATASET_ID,
            self.common.dataset_id.as_ref().filter(|_| keeps(InstanceMember::DatasetId)),
        )?;
        serialize_sub_attributes(&mut object, &self.common.sub_attributes, self.reification, self.walk)?;
        object.end()
    }
}
