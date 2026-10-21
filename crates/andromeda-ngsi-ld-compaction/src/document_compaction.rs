//! Compacting a written document: the entity node and the instances of a
//! write, of a Temporal Evolution fragment (clause 4.5.7), or of an entity the
//! broker assembles for a response.
//!
//! A written instance carries what a stored one does, and also what only a
//! document can: an omitted family or value, several instances of one
//! `datasetId`, `instanceId` and `deletedAt`, and the NGSI-LD Null of a
//! deleted member, each in the expanded shape the canonical codec writes.

use crate::{
    carrier_items,
    compacted::{Compacted, CompactedObject},
    expanded_item::{ExpandedItem, node_identifier},
    reserved_terms::reserved,
    undecidable::Undecidable,
    walk::{Walk, property_term},
};
use andromeda_ngsi_ld_entity::{
    document::{
        EntityDocument,
        attribute_member::AttributeMember,
        instance_document::InstanceDocument,
        slot::Slot,
        written_value::{ValueSlot, WrittenValue},
    },
    value_mode::Fragment,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, reference::NgsiLdReference, time::date_time::NgsiLdDateTime, uri::Uri};
use andromeda_ngsi_ld_vocabulary::NULL_URI;
use indexmap::IndexMap;
use jsonld::{BlankIdBuf, IriBuf, Term};
use std::slice::from_ref;

/// The items of one written member.
type WrittenItems<'a> = Vec<ExpandedItem<'a, Fragment>>;

impl Walk<'_> {
    /// Compact the entity node of a written document.
    pub(crate) fn compact_document<'a>(&mut self, document: &'a EntityDocument) -> Result<CompactedObject<'a>, Undecidable> {
        let mut result = CompactedObject::new();
        let id = document.id().map(|id| Term::Id(node_identifier(id.as_str())));
        let types: Vec<_> = document.types().iter().map(|ty| Term::Id(node_identifier(ty.as_str()))).collect();
        self.write_identity(&mut result, id.as_ref(), &types)?;
        let scope: Option<WrittenItems<'a>> = document.scope().map(|scope| match scope {
            Slot::Set(scope) => scope.as_slice().iter().map(|path| ExpandedItem::text(path.as_str())).collect(),
            Slot::Remove => vec![ExpandedItem::text(NULL_URI.as_str())],
        });
        if let Some(scope) = scope {
            self.compact_property(&mut result, &reserved().scope, &scope)?;
        }
        self.compact_date_time_slot(&mut result, &reserved().expires_at, document.expires_at())?;
        self.compact_members(&mut result, document.attributes())?;
        Ok(result)
    }

    /// Compact one written instance node.
    pub(crate) fn compact_written_instance<'a>(&mut self, instance: &'a InstanceDocument) -> Result<Compacted<'a>, Undecidable> {
        let mut result = CompactedObject::new();
        let family = instance.family().map_or(&[][..], |family| from_ref(reserved().family(family)));
        self.write_identity(&mut result, None, family)?;
        match instance.value() {
            ValueSlot::Absent => {}
            ValueSlot::Set(value) => self.compact_property(&mut result, reserved().carrier(value.carrier()), &written_items(value)?)?,
            ValueSlot::Remove(carrier) => {
                let deletion: WrittenItems<'a> = carrier_items::deletion(*carrier);
                self.compact_property(&mut result, reserved().carrier(*carrier), &deletion)?;
            }
        }
        self.compact_date_time_slot(&mut result, &reserved().observed_at, instance.observed_at())?;
        self.compact_date_time_slot(&mut result, &reserved().expires_at, instance.expires_at())?;
        let created_at = instance.created_at().map(|instant| instant.to_string());
        let modified_at = instance.modified_at().map(|instant| instant.to_string());
        self.compact_date_time(&mut result, &reserved().created_at, created_at.as_deref())?;
        self.compact_date_time(&mut result, &reserved().modified_at, modified_at.as_deref())?;
        let deleted_at = instance.deleted_at().map(|instant| instant.to_string());
        self.compact_date_time(&mut result, &reserved().deleted_at, deleted_at.as_deref())?;
        self.compact_reference(&mut result, &reserved().dataset_id, instance.dataset_id().map(Identifier::as_str))?;
        self.compact_reference(&mut result, &reserved().instance_id, instance.instance_id().map(Identifier::as_str))?;
        if let Some(unit_code) = instance.unit_code() {
            let text = match unit_code {
                Slot::Set(unit_code) => unit_code.as_str(),
                Slot::Remove => NULL_URI.as_str(),
            };
            let item: WrittenItems<'a> = vec![ExpandedItem::text(text)];
            self.compact_property(&mut result, &reserved().unit_code, &item)?;
        }
        let value_type = instance.value_type().map(|value_type| match value_type {
            Slot::Set(value_type) => value_type.as_str(),
            Slot::Remove => NULL_URI.as_str(),
        });
        self.compact_reference(&mut result, &reserved().value_type, value_type)?;
        if let Some(object_types) = instance.object_types() {
            let items: WrittenItems<'a> = match object_types {
                Slot::Set(object_types) => carrier_items::references(object_types.iter().map(NgsiLdReference::as_str)),
                Slot::Remove => carrier_items::references([NULL_URI.as_str()]),
            };
            self.compact_property(&mut result, &reserved().object_type, &items)?;
        }
        self.compact_seals(&mut result, instance.proof_seals())?;
        self.compact_members(&mut result, instance.sub_attributes())?;
        Ok(Compacted::Object(result))
    }

    /// Compact every written attribute member into `result`: the instances of
    /// each, or the NGSI-LD Null standing for the whole attribute.
    fn compact_members<'a>(&mut self, result: &mut CompactedObject<'a>, members: &'a IndexMap<AttributeName, AttributeMember>) -> Result<(), Undecidable> {
        for (name, member) in members {
            let Some(attribute) = property_term(name.as_str()) else {
                continue;
            };
            match member {
                AttributeMember::Remove => {
                    let null: WrittenItems<'a> = vec![ExpandedItem::text(NULL_URI.as_str())];
                    self.compact_property(result, &attribute, &null)?;
                }
                AttributeMember::Instances(instances) => {
                    let items: WrittenItems<'a> = instances.iter().map(ExpandedItem::WrittenInstance).collect();
                    self.compact_attribute(result, &attribute, &items)?;
                }
            }
        }
        Ok(())
    }

    /// Compact a `DateTime` member a document sets or deletes.
    fn compact_date_time_slot(
        &mut self,
        result: &mut CompactedObject<'_>,
        member: &Term<IriBuf, BlankIdBuf>,
        slot: Option<&Slot<NgsiLdDateTime>>,
    ) -> Result<(), Undecidable> {
        let text = slot.map(|slot| match slot {
            Slot::Set(instant) => instant.to_string(),
            Slot::Remove => NULL_URI.as_str().to_owned(),
        });
        self.compact_date_time(result, member, text.as_deref())
    }
}

/// The items a written value holds under its carrier.
fn written_items(value: &WrittenValue) -> Result<WrittenItems<'_>, Undecidable> {
    Ok(match value {
        WrittenValue::Value(values) => ExpandedItem::of_values(values.as_slice())?,
        WrittenValue::Geometry(geometry) => carrier_items::geometry(geometry),
        WrittenValue::Object(objects) => carrier_items::references(objects.iter().map(Uri::as_str)),
        WrittenValue::LanguageMap(entries) => carrier_items::language_entries(entries),
        WrittenValue::Vocab(vocab) => carrier_items::references(vocab.iter().map(NgsiLdReference::as_str)),
        WrittenValue::Json(json) => carrier_items::json_literals(json.as_slice())?,
        WrittenValue::ValueList(items) => carrier_items::value_list(items)?,
        WrittenValue::ObjectList(objects) => carrier_items::object_list(objects),
    })
}
