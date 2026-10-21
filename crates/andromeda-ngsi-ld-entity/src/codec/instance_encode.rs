//! Writing stored attributes and instances in their expanded form.
//!
//! The instance's `@type` is written first: a reader that streams the document
//! back knows the family before it meets the value, which is what lets a
//! geometry be read straight into its positions.

use crate::{
    attribute_instances::AttributeInstances,
    attributes::Attributes,
    codec::{
        carrier_encode::{
            JsonLiteralsOut,
            LanguageEntriesOut,
            ListObjectsOut,
            NamesOut,
            One,
            UrisOut,
            write_date_time_text,
            write_family,
            write_reference,
            write_text,
        },
        geometry_encode::GeoValueOut,
        value_encode::{ListOut, ValuesOut},
    },
    family_value::FamilyValue,
    instance::Instance,
    proof_seals::ProofSeals,
    value_mode::Stored,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use serde::{Serialize, Serializer, ser::SerializeMap};

/// Write every attribute of `attributes` as a member of the enclosing object.
pub(crate) fn write_attributes<M: SerializeMap>(map: &mut M, attributes: &Attributes) -> Result<(), M::Error> {
    attributes
        .iter()
        .try_for_each(|(name, instances)| map.serialize_entry(name.as_str(), &InstancesOut(instances)))
}

/// The instances of one attribute, in stored order.
struct InstancesOut<'a>(&'a AttributeInstances);

impl Serialize for InstancesOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|(dataset_id, instance)| InstanceOut { dataset_id, instance }))
    }
}

/// One stored instance.
struct InstanceOut<'a> {
    /// Its `datasetId`.
    dataset_id: Option<&'a Identifier>,
    /// The instance.
    instance: &'a Instance,
}

impl Serialize for InstanceOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let instance = self.instance;
        let value = instance.value();
        let mut map = serializer.serialize_map(None)?;
        write_family(&mut map, value.family().iri_str())?;
        write_family_value(&mut map, value)?;
        if let Some(observed_at) = instance.observed_at() {
            write_date_time_text(&mut map, properties::OBSERVED_AT.as_str(), &observed_at.to_string())?;
        }
        if let Some(expires_at) = instance.expires_at() {
            write_date_time_text(&mut map, properties::EXPIRES_AT.as_str(), &expires_at.to_string())?;
        }
        if let Some(created_at) = instance.created_at() {
            write_date_time_text(&mut map, properties::CREATED_AT.as_str(), &created_at.to_string())?;
        }
        if let Some(modified_at) = instance.modified_at() {
            write_date_time_text(&mut map, properties::MODIFIED_AT.as_str(), &modified_at.to_string())?;
        }
        if let Some(dataset_id) = self.dataset_id {
            write_reference(&mut map, properties::DATASET_ID.as_str(), dataset_id.as_str())?;
        }
        write_seals(&mut map, instance.proof_seals())?;
        write_attributes(&mut map, instance.sub_attributes())?;
        map.end()
    }
}

/// Write the data-integrity seals of clause 4.5.2.2.
pub(crate) fn write_seals<M: SerializeMap>(map: &mut M, seals: &ProofSeals) -> Result<(), M::Error> {
    if let Some(values) = seals.entity_id() {
        map.serialize_entry(properties::ENTITY_ID_SEALED.as_str(), &ValuesOut::<Stored>(values))?;
    }
    if let Some(values) = seals.entity_type() {
        map.serialize_entry(properties::ENTITY_TYPE_SEALED.as_str(), &ValuesOut::<Stored>(values))?;
    }
    Ok(())
}

/// Write a family's value carrier and the members only that family defines.
fn write_family_value<M: SerializeMap>(map: &mut M, value: &FamilyValue) -> Result<(), M::Error> {
    let key = value.family().value_key_str();
    match value {
        FamilyValue::Property { values, .. } => map.serialize_entry(key, &ValuesOut::<Stored>(values.as_slice()))?,
        FamilyValue::GeoProperty { geometry, .. } => map.serialize_entry(key, &One(GeoValueOut(geometry)))?,
        FamilyValue::Relationship { objects, .. } => map.serialize_entry(key, &UrisOut(objects.as_slice()))?,
        FamilyValue::LanguageProperty { entries, .. } => map.serialize_entry(key, &LanguageEntriesOut(entries))?,
        FamilyValue::VocabProperty { vocab, .. } => map.serialize_entry(key, &NamesOut(vocab.as_slice()))?,
        FamilyValue::JsonProperty { json, .. } => map.serialize_entry(key, &JsonLiteralsOut(json.as_slice()))?,
        FamilyValue::ListProperty { items, .. } => map.serialize_entry(key, &One(ListOut(ValuesOut::<Stored>(items))))?,
        FamilyValue::ListRelationship { objects, .. } => map.serialize_entry(key, &One(ListOut(ListObjectsOut(objects))))?,
    }
    if let Some(unit_code) = value.unit_code() {
        write_text(map, properties::UNIT_CODE.as_str(), unit_code.as_str())?;
    }
    if let Some(value_type) = value.value_type() {
        write_reference(map, properties::VALUE_TYPE.as_str(), value_type.as_str())?;
    }
    if !value.object_types().is_empty() {
        map.serialize_entry(properties::OBJECT_TYPE.as_str(), &NamesOut(value.object_types()))?;
    }
    Ok(())
}
