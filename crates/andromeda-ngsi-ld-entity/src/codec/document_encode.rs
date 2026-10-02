//! Writing written attribute members and instances in their expanded form.
//!
//! A written instance is written as the expansion of the request it came from
//! would read: its family when it declared one, its value or the deletion
//! encoding of its carrier, and each member it set or deleted.

use crate::{
    codec::{
        carrier_encode::{
            JsonLiteralsOut,
            LanguageEntriesOut,
            ListObjectsOut,
            NamesOut,
            One,
            UrisOut,
            write_carrier_deletion,
            write_date_time,
            write_date_time_text,
            write_family,
            write_reference,
            write_text,
        },
        geometry_encode::GeoValueOut,
        instance_encode::write_seals,
        value_encode::{ListOut, NullValueOut, ValuesOut},
    },
    document::{
        attribute_member::AttributeMember,
        instance_document::InstanceDocument,
        slot::Slot,
        written_value::{ValueSlot, WrittenValue},
    },
    value_mode::Fragment,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::{NULL_URI, expanded::properties};
use indexmap::IndexMap;
use serde::{Serialize, Serializer, ser::SerializeMap};

/// Write every written attribute member as a member of the enclosing object.
pub(crate) fn write_members<M: SerializeMap>(map: &mut M, members: &IndexMap<AttributeName, AttributeMember>) -> Result<(), M::Error> {
    members
        .iter()
        .try_for_each(|(name, member)| map.serialize_entry(name.as_str(), &MemberOut(member)))
}

/// One written attribute member.
struct MemberOut<'a>(&'a AttributeMember);

impl Serialize for MemberOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            AttributeMember::Remove => One(NullValueOut).serialize(serializer),
            AttributeMember::Instances(instances) => serializer.collect_seq(instances.iter().map(InstanceDocumentOut)),
        }
    }
}

/// One written instance.
pub(crate) struct InstanceDocumentOut<'a>(pub(crate) &'a InstanceDocument);

impl Serialize for InstanceDocumentOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let instance = self.0;
        let mut map = serializer.serialize_map(None)?;
        if let Some(family) = instance.family() {
            write_family(&mut map, family.iri_str())?;
        }
        match instance.value() {
            ValueSlot::Absent => {}
            ValueSlot::Set(value) => write_written_value(&mut map, value)?,
            ValueSlot::Remove(carrier) => write_carrier_deletion(&mut map, *carrier)?,
        }
        write_date_time(&mut map, properties::OBSERVED_AT.as_str(), instance.observed_at())?;
        write_date_time(&mut map, properties::EXPIRES_AT.as_str(), instance.expires_at())?;
        if let Some(created_at) = instance.created_at() {
            write_date_time_text(&mut map, properties::CREATED_AT.as_str(), &created_at.to_string())?;
        }
        if let Some(modified_at) = instance.modified_at() {
            write_date_time_text(&mut map, properties::MODIFIED_AT.as_str(), &modified_at.to_string())?;
        }
        if let Some(deleted_at) = instance.deleted_at() {
            write_date_time_text(&mut map, properties::DELETED_AT.as_str(), deleted_at.as_str())?;
        }
        if let Some(dataset_id) = instance.dataset_id() {
            write_reference(&mut map, properties::DATASET_ID.as_str(), dataset_id.as_str())?;
        }
        if let Some(instance_id) = instance.instance_id() {
            write_reference(&mut map, properties::INSTANCE_ID.as_str(), instance_id.as_str())?;
        }
        match instance.unit_code() {
            Some(Slot::Set(unit_code)) => write_text(&mut map, properties::UNIT_CODE.as_str(), unit_code.as_str())?,
            Some(Slot::Remove) => write_text(&mut map, properties::UNIT_CODE.as_str(), NULL_URI.as_str())?,
            None => {}
        }
        match instance.value_type() {
            Some(Slot::Set(value_type)) => write_reference(&mut map, properties::VALUE_TYPE.as_str(), value_type.as_str())?,
            Some(Slot::Remove) => write_reference(&mut map, properties::VALUE_TYPE.as_str(), NULL_URI.as_str())?,
            None => {}
        }
        match instance.object_types() {
            Some(Slot::Set(object_types)) => map.serialize_entry(properties::OBJECT_TYPE.as_str(), &NamesOut(object_types))?,
            Some(Slot::Remove) => write_reference(&mut map, properties::OBJECT_TYPE.as_str(), NULL_URI.as_str())?,
            None => {}
        }
        write_seals(&mut map, instance.proof_seals())?;
        write_members(&mut map, instance.sub_attributes())?;
        map.end()
    }
}

/// Write a written value under its carrier.
fn write_written_value<M: SerializeMap>(map: &mut M, value: &WrittenValue) -> Result<(), M::Error> {
    let key = value.carrier().iri().into_inner();
    match value {
        WrittenValue::Value(values) => map.serialize_entry(key, &ValuesOut::<Fragment>(values.as_slice())),
        WrittenValue::Geometry(geometry) => map.serialize_entry(key, &One(GeoValueOut(geometry))),
        WrittenValue::Object(objects) => map.serialize_entry(key, &UrisOut(objects.as_slice())),
        WrittenValue::LanguageMap(entries) => map.serialize_entry(key, &LanguageEntriesOut(entries)),
        WrittenValue::Vocab(vocab) => map.serialize_entry(key, &NamesOut(vocab.as_slice())),
        WrittenValue::Json(json) => map.serialize_entry(key, &JsonLiteralsOut(json.as_slice())),
        WrittenValue::ValueList(items) => map.serialize_entry(key, &One(ListOut(ValuesOut::<Fragment>(items)))),
        WrittenValue::ObjectList(objects) => map.serialize_entry(key, &One(ListOut(ListObjectsOut(objects)))),
    }
}
