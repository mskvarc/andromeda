//! Compacting the attributes of entity state, instance by instance.
//!
//! A stored instance is the node `{"@type": [family], <carrier>, <members>,
//! <sub-attributes>}` (clause 4.5); its `datasetId` is the key the model holds
//! it under, written back as the member it was stored as. The clause 4.8
//! `createdAt` and `modifiedAt` of a top-level instance are left out of a
//! representation that did not ask for `sysAttrs` (Table 5.2.14-1); those of a
//! sub-attribute instance are written as stored.

use crate::{
    carrier_items,
    compacted::{Compacted, CompactedObject},
    expanded_item::{ExpandedItem, StoredInstance},
    reserved_terms::reserved,
    undecidable::Undecidable,
    walk::{Walk, property_term},
};
use andromeda_ngsi_ld_entity::{
    attributes::Attributes,
    date_time_text::DateTimeText,
    document::value_carrier::ValueCarrier,
    family_value::FamilyValue,
    proof_seals::ProofSeals,
    value_mode::Stored,
};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, reference::NgsiLdReference, uri::Uri};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use jsonld::{BlankIdBuf, IriBuf, Term};
use std::slice::from_ref;

/// The items of one stored member.
type StoredItems<'a> = Vec<ExpandedItem<'a, Stored>>;

impl Walk<'_> {
    /// Compact every attribute of `attributes` into `result`.
    pub(crate) fn compact_stored_attributes<'a>(
        &mut self,
        result: &mut CompactedObject<'a>,
        attributes: &'a Attributes,
        system_times: SysAttrsInclusion,
    ) -> Result<(), Undecidable> {
        for (name, instances) in attributes.iter() {
            let Some(attribute) = property_term(name.as_str()) else {
                continue;
            };
            let items: StoredItems<'a> = instances
                .iter()
                .map(|(dataset_id, instance)| {
                    ExpandedItem::StoredInstance(StoredInstance {
                        instance,
                        dataset_id,
                        system_times,
                    })
                })
                .collect();
            self.compact_attribute(result, &attribute, &items)?;
        }
        Ok(())
    }

    /// Compact one stored instance node.
    pub(crate) fn compact_stored_instance<'a>(&mut self, stored: &StoredInstance<'a>) -> Result<Compacted<'a>, Undecidable> {
        let instance = stored.instance;
        let value = instance.value();
        let mut result = CompactedObject::new();
        self.write_identity(&mut result, None, from_ref(reserved().family(value.family())))?;
        self.compact_family_value(&mut result, value)?;
        self.compact_date_time(&mut result, &reserved().observed_at, instance.observed_at().map(DateTimeText::as_str))?;
        self.compact_date_time(&mut result, &reserved().expires_at, instance.expires_at().map(DateTimeText::as_str))?;
        if stored.system_times == SysAttrsInclusion::Include {
            let created_at = instance.created_at().map(|instant| instant.to_string());
            let modified_at = instance.modified_at().map(|instant| instant.to_string());
            self.compact_date_time(&mut result, &reserved().created_at, created_at.as_deref())?;
            self.compact_date_time(&mut result, &reserved().modified_at, modified_at.as_deref())?;
        }
        self.compact_reference(&mut result, &reserved().dataset_id, stored.dataset_id.map(Identifier::as_str))?;
        self.compact_seals(&mut result, instance.proof_seals())?;
        self.compact_stored_attributes(&mut result, instance.sub_attributes(), stored.system_times)?;
        Ok(Compacted::Object(result))
    }

    /// Compact the `entityIdSealed` and `entityTypeSealed` members (clause
    /// 4.5.2.2).
    pub(crate) fn compact_seals<'a>(&mut self, result: &mut CompactedObject<'a>, seals: &'a ProofSeals) -> Result<(), Undecidable> {
        if let Some(values) = seals.entity_id() {
            self.compact_property(result, &reserved().entity_id_sealed, &ExpandedItem::of_values(values)?)?;
        }
        if let Some(values) = seals.entity_type() {
            self.compact_property(result, &reserved().entity_type_sealed, &ExpandedItem::of_values(values)?)?;
        }
        Ok(())
    }

    /// Compact a `DateTime` member, when present.
    pub(crate) fn compact_date_time<'a>(
        &mut self,
        result: &mut CompactedObject<'a>,
        member: &Term<IriBuf, BlankIdBuf>,
        text: Option<&str>,
    ) -> Result<(), Undecidable> {
        let Some(text) = text else {
            return Ok(());
        };
        // clone: the value object owns its datatype IRI
        let item: [ExpandedItem<'a, Stored>; 1] = [ExpandedItem::typed_text(text, reserved().date_time.clone())];
        self.compact_property(result, member, &item)
    }

    /// Compact a member holding one node reference, when present.
    pub(crate) fn compact_reference<'a>(
        &mut self,
        result: &mut CompactedObject<'a>,
        member: &Term<IriBuf, BlankIdBuf>,
        iri: Option<&str>,
    ) -> Result<(), Undecidable> {
        let Some(iri) = iri else {
            return Ok(());
        };
        let item: [ExpandedItem<'a, Stored>; 1] = [ExpandedItem::reference(iri)];
        self.compact_property(result, member, &item)
    }

    /// Compact a family's value carrier and the members only that family
    /// defines.
    fn compact_family_value<'a>(&mut self, result: &mut CompactedObject<'a>, value: &'a FamilyValue) -> Result<(), Undecidable> {
        let carrier = reserved().carrier(ValueCarrier::of(value.family()));
        let items: StoredItems<'a> = match value {
            FamilyValue::Property { values, .. } => ExpandedItem::of_values(values.as_slice())?,
            FamilyValue::GeoProperty { geometry, .. } => carrier_items::geometry(geometry),
            FamilyValue::Relationship { objects, .. } => carrier_items::references(objects.iter().map(Uri::as_str)),
            FamilyValue::LanguageProperty { entries, .. } => carrier_items::language_entries(entries),
            FamilyValue::VocabProperty { vocab, .. } => carrier_items::references(vocab.iter().map(NgsiLdReference::as_str)),
            FamilyValue::JsonProperty { json, .. } => carrier_items::json_literals(json.as_slice())?,
            FamilyValue::ListProperty { items, .. } => carrier_items::value_list(items)?,
            FamilyValue::ListRelationship { objects, .. } => carrier_items::object_list(objects),
        };
        self.compact_property(result, carrier, &items)?;
        if let Some(unit_code) = value.unit_code() {
            let item: StoredItems<'a> = vec![ExpandedItem::text(unit_code.as_str())];
            self.compact_property(result, &reserved().unit_code, &item)?;
        }
        self.compact_reference(result, &reserved().value_type, value.value_type().map(NgsiLdReference::as_str))?;
        if !value.object_types().is_empty() {
            let items: StoredItems<'a> = carrier_items::references(value.object_types().iter().map(NgsiLdReference::as_str));
            self.compact_property(result, &reserved().object_type, &items)?;
        }
        Ok(())
    }
}
