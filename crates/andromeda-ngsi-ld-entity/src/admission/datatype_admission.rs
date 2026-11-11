//! Holding every literal to the datatype it is declared as, at every depth.
//!
//! A literal is declared by its JSON-LD `@type` and by the `valueType` of the
//! attribute it is a value of (clause 4.5.2, annex C.6). `valueType` declares
//! the datatype of every primitive member of the value it sits beside: the
//! single value, each value of a multi-valued Property, each `ListProperty`
//! item. Inside a structured value only a member's own `@type` declares
//! anything. A literal outside a recognised datatype, a `valueType` that is no
//! absolute IRI and a `LanguageProperty` whose `valueType` is not
//! `langString` (Table 5.2.32-1) are refused. A sub-attribute is an attribute
//! of its instance (clause 4.5.2.2), so the rule holds at every depth, on the
//! Core API and the Temporal API alike.

use crate::{
    document::{
        EntityDocument,
        instance_document::InstanceDocument,
        slot::Slot,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::ExpandedValue,
    family_value::FamilyValue,
    instance::Instance,
    literal::Literal,
    refusal::EntityRefusal,
    value_mode::ValueMode,
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    datatype::{
        declaration::LiteralDeclaration,
        declared_value::DeclaredValue,
        recognised::{DatatypeError, RecognisedDatatype, admit_language_map_value_type},
    },
    reference::NgsiLdReference,
};

impl EntityDocument {
    /// Hold every literal of the document to the datatype it is declared as.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRefusal::Datatype`] for the first literal or `valueType`
    /// refused.
    pub fn admit_datatypes(&self) -> Result<(), EntityRefusal> {
        self.attributes()
            .iter()
            .try_for_each(|(name, member)| member.instances().iter().try_for_each(|instance| admit_document(name, instance)))
    }
}

impl InstanceDocument {
    /// Hold every literal of the instance written for `attribute` to the
    /// datatype it is declared as.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRefusal::Datatype`] for the first literal or `valueType`
    /// refused.
    pub fn admit_datatypes(&self, attribute: &AttributeName) -> Result<(), EntityRefusal> {
        admit_document(attribute, self)
    }
}

impl Instance {
    /// Hold the value of this instance of `attribute` to the datatype it is
    /// declared as, once a merge has put a written value beside a held
    /// `valueType` or a written `valueType` beside a held value.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRefusal::Datatype`] for the first literal or `valueType`
    /// refused.
    pub fn admit_datatypes(&self, attribute: &AttributeName) -> Result<(), EntityRefusal> {
        let refused = |source| refusal(attribute, source);
        let value = self.value();
        let value_type = value.value_type();
        if let Some(value_type) = value_type {
            admit_value_type(value_type, Some(self.family())).map_err(refused)?;
        }
        let values = match value {
            FamilyValue::Property { values, .. } => values.as_slice(),
            FamilyValue::Proof(proof) => proof.values(),
            FamilyValue::ListProperty { items, .. } => items.as_slice(),
            FamilyValue::GeoProperty { .. }
            | FamilyValue::Relationship { .. }
            | FamilyValue::LanguageProperty { .. }
            | FamilyValue::VocabProperty { .. }
            | FamilyValue::JsonProperty { .. }
            | FamilyValue::ListRelationship { .. } => &[],
        };
        values.iter().try_for_each(|item| admit_value(item, value_type)).map_err(refused)
    }
}

/// [`InstanceDocument::admit_datatypes`], here and on every sub-attribute below.
fn admit_document(name: &AttributeName, instance: &InstanceDocument) -> Result<(), EntityRefusal> {
    let refused = |source| refusal(name, source);
    let value_type = match instance.value_type() {
        Some(Slot::Set(value_type)) => Some(value_type),
        Some(Slot::Remove) | None => None,
    };
    let values = match instance.value() {
        ValueSlot::Set(WrittenValue::Value(values) | WrittenValue::ValueList(values)) => values.as_slice(),
        ValueSlot::Set(
            WrittenValue::Json(_)
            | WrittenValue::Geometry(_)
            | WrittenValue::Object(_)
            | WrittenValue::LanguageMap(_)
            | WrittenValue::Vocab(_)
            | WrittenValue::ObjectList(_),
        )
        | ValueSlot::Absent
        | ValueSlot::Remove(_) => &[],
    };
    if let Some(value_type) = value_type {
        admit_value_type(value_type, instance.family()).map_err(refused)?;
    }
    values.iter().try_for_each(|value| admit_value(value, value_type)).map_err(refused)?;
    instance
        .sub_attributes()
        .iter()
        .try_for_each(|(sub_name, member)| member.instances().iter().try_for_each(|sub| admit_document(sub_name, sub)))
}

/// A `valueType` is an absolute IRI, and `langString` on a `LanguageProperty`
/// of the family `family` (`None` for a fragment declaring none).
fn admit_value_type(value_type: &NgsiLdReference, family: Option<AttributeType>) -> Result<(), DatatypeError> {
    match family {
        Some(AttributeType::LanguageProperty) => admit_language_map_value_type(value_type),
        Some(
            AttributeType::Property
            | AttributeType::Relationship
            | AttributeType::GeoProperty
            | AttributeType::VocabProperty
            | AttributeType::JsonProperty
            | AttributeType::ListProperty
            | AttributeType::ListRelationship,
        )
        | None => RecognisedDatatype::of(value_type).map(drop),
    }
}

/// One value and everything inside it: `value_type` declares a primitive
/// member, a list item included, and nothing inside a structured value.
fn admit_value<M: ValueMode>(value: &ExpandedValue<M>, value_type: Option<&NgsiLdReference>) -> Result<(), DatatypeError> {
    match value {
        ExpandedValue::Literal(literal) => admit_literal(literal, value_type),
        ExpandedValue::Json(_) => Ok(()),
        ExpandedValue::List(items) => items.iter().try_for_each(|item| admit_value(item, value_type)),
        ExpandedValue::Node(node) => node
            .members()
            .values()
            .try_for_each(|member| M::held_values(member).iter().try_for_each(|item| admit_value(item, None))),
    }
}

/// One literal against its own datatype and the `valueType` beside it, and
/// the value the deciding one reads from it, which the broker must hold: a
/// declared number past the range of the store is refused here, not at the
/// write.
fn admit_literal(literal: &Literal, value_type: Option<&NgsiLdReference>) -> Result<(), DatatypeError> {
    let declaration = LiteralDeclaration::new(literal.datatype(), value_type);
    declaration.time_form()?;
    let primitive = literal.value().primitive();
    let declared = match (literal.datatype(), value_type) {
        (Some(datatype), Some(value_type)) if datatype == value_type => [Some(datatype), None],
        (datatype, value_type) => [datatype, value_type],
    };
    declared
        .into_iter()
        .flatten()
        .try_for_each(|datatype| match RecognisedDatatype::of(datatype)? {
            Some(recognised) => recognised.admits(primitive),
            None => Ok(()),
        })?;
    DeclaredValue::of(declaration, primitive).transpose().map(drop)
}

/// The refusal of a literal or `valueType` of `attribute`.
fn refusal(attribute: &AttributeName, source: DatatypeError) -> EntityRefusal {
    EntityRefusal::Datatype {
        // clone: the refusal outlives the borrowed request.
        attribute: attribute.clone(),
        source,
    }
}
