//! Refusing a written text the broker cannot store.
//!
//! JSON admits U+0000 in a string (IETF RFC 8259 section 7) and ETSI GS CIM
//! 009 v1.9.1 sets no rule against it, but PostgreSQL holds the character
//! neither in `text` nor in `jsonb`, so a write carrying it could only fail at
//! its commit. It is refused with the clause 5.5.4 validation instead, as input
//! the operation cannot take: a single write answers `BadRequestData`, and a
//! batch reports the entity that carried it and writes the others (clause
//! 5.6.7.4).
//!
//! Every text a client chooses freely is read: string literals at every depth,
//! the identifiers, member names and `@type` entries of a structured value,
//! language map entries, JSON literals, the foreign members of a geometry and
//! its clause 4.7.2 string form, and the proof seals of clause 4.5.2.2.
//! Entity and attribute names, identifiers and IRIs are held to grammars that
//! exclude the character already.

use crate::{
    document::{
        EntityDocument,
        instance_document::InstanceDocument,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::ExpandedValue,
    geo_value::GeoValue,
    literal::LiteralValue,
    refusal::EntityRefusal,
    value_mode::{FragmentText, ValueMode},
};
use andromeda_json_literal::json_text::holds_nul_character;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// The character no stored text holds.
const NUL: char = '\0';

impl EntityDocument {
    /// Hold every text of the document to what the broker can store.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRefusal::NulCharacter`] for the first attribute or
    /// sub-attribute whose value holds U+0000.
    pub fn admit_stored_text(&self) -> Result<(), EntityRefusal> {
        self.attributes()
            .iter()
            .try_for_each(|(name, member)| member.instances().iter().try_for_each(|instance| admit_instance(name, instance)))
    }
}

impl InstanceDocument {
    /// Hold every text of the instance written for `attribute` to what the
    /// broker can store.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRefusal::NulCharacter`] for the instance or the first
    /// sub-attribute whose value holds U+0000.
    pub fn admit_stored_text(&self, attribute: &AttributeName) -> Result<(), EntityRefusal> {
        admit_instance(attribute, self)
    }
}

/// [`InstanceDocument::admit_stored_text`], here and on every sub-attribute below.
fn admit_instance(name: &AttributeName, instance: &InstanceDocument) -> Result<(), EntityRefusal> {
    let value_holds_nul = match instance.value() {
        ValueSlot::Set(value) => written_holds_nul(value),
        ValueSlot::Absent | ValueSlot::Remove(_) => false,
    };
    let seals = instance.proof_seals();
    let seal_holds_nul = seals.entity_id().into_iter().chain(seals.entity_type()).flatten().any(holds_nul);
    if value_holds_nul || seal_holds_nul {
        return Err(EntityRefusal::NulCharacter {
            attribute: name.clone(), // clone: the refusal names the attribute the document keeps
        });
    }
    instance
        .sub_attributes()
        .iter()
        .try_for_each(|(sub_name, member)| member.instances().iter().try_for_each(|sub| admit_instance(sub_name, sub)))
}

/// Whether the value an instance writes holds U+0000.
fn written_holds_nul(value: &WrittenValue) -> bool {
    match value {
        WrittenValue::Value(values) | WrittenValue::ValueList(values) => values.iter().any(holds_nul),
        WrittenValue::Geometry(geometry) => geometry_holds_nul(geometry),
        WrittenValue::LanguageMap(entries) => entries.entries().iter().any(|entry| match entry.text() {
            FragmentText::Text(text) => text.contains(NUL),
            FragmentText::Remove => false,
        }),
        WrittenValue::Json(literals) => literals.as_slice().iter().any(|literal| holds_nul_character(literal.text())),
        WrittenValue::Object(_) | WrittenValue::Vocab(_) | WrittenValue::ObjectList(_) => false,
    }
}

/// Whether a geometry's foreign members or its string form hold U+0000.
fn geometry_holds_nul(geometry: &GeoValue) -> bool {
    match geometry {
        GeoValue::Encoded(encoded) => holds_nul_character(encoded.as_str()),
        GeoValue::Geometry(value) => value.members().is_some_and(|members| {
            members
                .foreign()
                .iter()
                .any(|(name, values)| name.as_str().contains(NUL) || values.iter().any(holds_nul))
        }),
    }
}

/// Whether one expanded value, or any value below it, holds U+0000.
fn holds_nul<M: ValueMode>(value: &ExpandedValue<M>) -> bool {
    match value {
        ExpandedValue::Literal(literal) => match literal.value() {
            LiteralValue::String(text) => text.contains(NUL),
            LiteralValue::Number(_) | LiteralValue::Boolean(_) => false,
        },
        ExpandedValue::Json(literal) => holds_nul_character(literal.text()),
        ExpandedValue::List(items) => items.iter().any(holds_nul),
        ExpandedValue::Node(node) => {
            node.id().is_some_and(|id| id.as_str().contains(NUL))
                || node.types().iter().any(|entry| entry.as_str().contains(NUL))
                || node
                    .members()
                    .iter()
                    .any(|(name, member)| name.as_str().contains(NUL) || M::held_values(member).iter().any(holds_nul))
        }
    }
}
