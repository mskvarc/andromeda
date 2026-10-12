//! Reading entity state as the written document it is.
//!
//! Every state is a document a write could have carried: the same members,
//! each set, none deleted. A reader that takes documents, such as the Temporal
//! Evolution a Core write records (clause 4.3.1) or the request an insert that
//! lost a race is retried as, reads a state through this conversion instead
//! of encoding it and decoding the text again; the result is the document that
//! decoding gives, member for member.

use crate::{
    attribute_instances::AttributeInstances,
    attributes::Attributes,
    document::{
        EntityDocument,
        attribute_member::AttributeMember,
        instance_document::InstanceDocument,
        slot::Slot,
        written_value::{ValueSlot, WrittenValue},
    },
    entity_state::EntityState,
    expanded_value::ExpandedValue,
    family_value::FamilyValue,
    instance::Instance,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use indexmap::IndexMap;

impl From<&EntityState> for EntityDocument {
    fn from(state: &EntityState) -> EntityDocument {
        EntityDocument::new(
            Some(state.id().clone()), // clone: the document owns what it names, the state stays shared
            state.types().as_slice().to_vec(),
            state.scope().cloned().map(Slot::Set),      // clone: as above
            state.expires_at().cloned().map(Slot::Set), // clone: as above
            state.attributes().written_members(),
        )
    }
}

impl InstanceDocument {
    /// The written instance a stored one is, under its `datasetId`.
    #[must_use]
    pub fn of_stored(dataset_id: Option<&Identifier>, instance: &Instance) -> InstanceDocument {
        let value = instance.value();
        let object_types = value.object_types();
        InstanceDocument::builder()
            .family(Some(value.family()))
            .dataset_id(dataset_id.cloned()) // clone: the document owns its identity, the state stays shared
            .value(ValueSlot::Set(written_value(value)))
            .observed_at(instance.observed_at().cloned().map(Slot::Set)) // clone: as above
            .expires_at(instance.expires_at().cloned().map(Slot::Set)) // clone: as above
            .unit_code(value.unit_code().map(Slot::Set))
            .value_type(value.value_type().cloned().map(Slot::Set)) // clone: as above
            .object_types((!object_types.is_empty()).then(|| Slot::Set(object_types.to_vec())))
            .created_at(instance.created_at())
            .modified_at(instance.modified_at())
            .instance_id(instance.instance_id().cloned()) // clone: as above
            .proof_seals(instance.proof_seals().clone()) // clone: as above
            .sub_attributes(instance.sub_attributes().written_members())
            .build()
    }
}

impl Attributes {
    /// Every stored attribute as its written member, in stored order.
    #[must_use]
    pub fn written_members(&self) -> IndexMap<AttributeName, AttributeMember> {
        self.iter()
            .map(|(name, instances)| (name.clone(), instance_members(instances))) // clone: the document owns its member names
            .collect()
    }
}

/// The instances of one stored attribute, in stored order.
fn instance_members(instances: &AttributeInstances) -> AttributeMember {
    AttributeMember::Instances(
        instances
            .iter()
            .map(|(dataset_id, instance)| InstanceDocument::of_stored(dataset_id, instance))
            .collect(),
    )
}

/// The written value a stored family value is, under its family's carrier.
fn written_value(value: &FamilyValue) -> WrittenValue {
    // clone: every arm copies the stored value into the document, which owns it
    match value {
        FamilyValue::Property { values, .. } => WrittenValue::Value(values.iter().map(ExpandedValue::to_fragment).collect()),
        FamilyValue::GeoProperty { geometry, .. } => WrittenValue::Geometry(geometry.clone()),
        FamilyValue::Relationship { objects, .. } => WrittenValue::Object(objects.clone()),
        FamilyValue::LanguageProperty { entries, .. } => WrittenValue::LanguageMap(entries.to_fragment()),
        FamilyValue::VocabProperty { vocab, .. } => WrittenValue::Vocab(vocab.clone()),
        FamilyValue::JsonProperty { json, .. } => WrittenValue::Json(json.clone()),
        FamilyValue::ListProperty { items, .. } => WrittenValue::ValueList(items.iter().map(ExpandedValue::to_fragment).collect()),
        FamilyValue::ListRelationship { objects, .. } => WrittenValue::ObjectList(objects.clone()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        codec::stored_document_fixtures::{every_family_document, list_relationship_spellings_document, stored_order_document},
        document::EntityDocument,
        entity_state::EntityState,
    };
    use serde_json::Value;

    /// The document decoding the state's own encoding gives.
    fn decoded_again(state: &EntityState) -> EntityDocument {
        EntityDocument::from_slice(&state.to_vec().expect("the state encodes")).expect("the encoding decodes as a document")
    }

    fn state_of(document: &Value) -> EntityState {
        EntityState::from_slice(&serde_json::to_vec(document).expect("a JSON document")).expect("a stored entity")
    }

    #[test]
    fn every_family_with_every_optional_member_reads_as_its_decoded_encoding() {
        let state = state_of(&every_family_document());
        pretty_assertions::assert_eq!(EntityDocument::from(&state), decoded_again(&state));
    }

    #[test]
    fn instances_read_in_stored_order_as_their_decoded_encoding() {
        let state = state_of(&stored_order_document());
        pretty_assertions::assert_eq!(EntityDocument::from(&state), decoded_again(&state));
    }

    #[test]
    fn object_list_spellings_read_as_their_decoded_encoding() {
        let state = state_of(&list_relationship_spellings_document());
        pretty_assertions::assert_eq!(EntityDocument::from(&state), decoded_again(&state));
    }
}
