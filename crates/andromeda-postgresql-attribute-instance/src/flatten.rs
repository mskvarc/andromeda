//! Flattening one attribute instance into its stored members, and its
//! sub-attribute tree into `sub_attribute` elements.
//!
//! A sub-attribute tree becomes elements in pre-order: every element before its
//! children, siblings and the dataset instances of one sub-attribute in written
//! order, each naming its parent by array position (0 for the instance that
//! holds the tree).
//!
//! A value lands in the typed column or field its shape names: one scalar in
//! its own column, everything else in `value_object` as the structured value
//! codec writes it for history. The instance must be one a write commits (its
//! values in their committed spelling), since storage keeps nothing else.

use crate::{
    name_keys::NameKeys,
    stored_json::StoredJson,
    stored_members::InstanceMembers,
    stored_value::{LiteralAnnotation, StoredValue},
    sub_attribute::SubAttributeElement,
};
use andromeda_ngsi_ld_entity::{
    attributes::Attributes,
    date_time_text::DateTimeText,
    expanded_value::ExpandedValue,
    family_value::FamilyValue,
    geo_value::GeoValue,
    instance::Instance,
    list_object::ListObject,
    literal::{Literal, LiteralValue},
    system_instant::SystemInstant,
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    identifier::Identifier,
    reference::NgsiLdReference,
    rfc3339::sub_second,
    time_value::TimeValue,
};
use andromeda_ngsi_ld_model::temporal::structured_value::StructuredValue;
use andromeda_postgresql_codes::attribute_kind_code::stored_attribute_kind;
use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
use serde::Serialize;
use serde_json::value::to_raw_value;
use thiserror::Error;

/// An Entity's attributes that cannot be written as rows.
#[derive(Debug, Error)]
pub enum FlattenError {
    /// An attribute name has no dictionary key.
    #[error("the attribute '{0}' has no dictionary key")]
    UnresolvedAttribute(AttributeName),
    /// A datatype, `valueType` or `objectType` IRI has no dictionary key.
    #[error("the type '{0}' has no dictionary key")]
    UnresolvedType(NgsiLdReference),
    /// A top-level instance carries no system times; a write stamps every one
    /// (clause 4.8) before it commits.
    #[error("an instance of '{0}' carries no system times")]
    Unstamped(AttributeName),
    /// A value is not in its committed spelling: storage keeps the instant of
    /// a `DateTime` and the target of a list item, and would read back another
    /// state than the one flattened.
    #[error("an instance of '{0}' holds a value in a spelling storage does not keep")]
    UncommittedSpelling(AttributeName),
    /// A `JsonProperty` holds several JSON literals, which the structured value
    /// codec stores one of.
    #[error("an instance of '{0}' holds several JSON literals")]
    SeveralJsonLiterals(AttributeName),
    /// An instance carries an `instanceId`, which only a Temporal Evolution
    /// holds (Table 5.2.5-2).
    #[error("an instance of '{0}' carries an instanceId")]
    InstanceIdentifier(AttributeName),
    /// An Entity holds more attribute instances than a row ordinal counts, or
    /// an instance more sub-attribute instances than an element position does.
    #[error("an instance of '{0}' holds more instances than storage numbers")]
    TooManyInstances(AttributeName),
    /// A structured value, geometry members or seals could not be written as
    /// JSON, which the model never produces.
    #[error("a stored JSON piece could not be written: {0}")]
    Json(#[from] serde_json::Error),
}

/// Append the instances of `attributes`, sub-attributes of the element at
/// `parent`, and their descendants to `elements` in pre-order.
///
/// # Errors
///
/// Returns [`FlattenError`] when a name has no key in `keys`, or when an
/// instance is not one a write commits.
pub fn push_elements(attributes: &Attributes, parent: i32, keys: &NameKeys, elements: &mut Vec<SubAttributeElement>) -> Result<(), FlattenError> {
    for (name, instances) in attributes.iter() {
        for (dataset_id, instance) in instances.iter() {
            elements.push(SubAttributeElement {
                parent,
                members: members(name, dataset_id, instance, keys)?,
                created_at: instance.created_at().map(SystemInstant::get),
                modified_at: instance.modified_at().map(SystemInstant::get),
            });
            // clone: the refusal owns the name it reports.
            let position = i32::try_from(elements.len()).map_err(|_| FlattenError::TooManyInstances(name.clone()))?;
            push_elements(instance.sub_attributes(), position, keys, elements)?;
        }
    }
    Ok(())
}

/// The members of one instance of `name`.
///
/// # Errors
///
/// Returns [`FlattenError`] when a name has no key in `keys`, or when the
/// instance is not one a write commits.
pub fn members(name: &AttributeName, dataset_id: Option<&Identifier>, instance: &Instance, keys: &NameKeys) -> Result<InstanceMembers, FlattenError> {
    if instance.instance_id().is_some() {
        // clone: the refusal owns the name it reports.
        return Err(FlattenError::InstanceIdentifier(name.clone()));
    }
    let committed = |text: &DateTimeText| DateTimeText::from_instant(text.instant()) == *text;
    if !instance.observed_at().is_none_or(committed) || !instance.expires_at().is_none_or(committed) {
        // clone: the refusal owns the name it reports.
        return Err(FlattenError::UncommittedSpelling(name.clone()));
    }
    let family = instance.value();
    // clone: the refusal owns the name it reports.
    let type_key = |reference: &NgsiLdReference| keys.type_name(reference).ok_or_else(|| FlattenError::UnresolvedType(reference.clone()));
    Ok(InstanceMembers {
        // clone: the refusal owns the name it reports.
        attribute_name_key: keys.attribute(name).ok_or_else(|| FlattenError::UnresolvedAttribute(name.clone()))?,
        dataset_id: dataset_id.cloned(), // clone: the row owns its datasetId
        attribute_kind: stored_attribute_kind(instance.family()),
        value: stored_value(name, family, keys)?,
        value_type_key: family.value_type().map(type_key).transpose()?,
        object_type_keys: family.object_types().iter().map(type_key).collect::<Result<_, _>>()?,
        unit_code: family.unit_code(),
        observed_at: instance.observed_at().map(DateTimeText::instant),
        expires_at: instance.expires_at().map(DateTimeText::instant),
        proof_seals: if instance.proof_seals().is_empty() {
            None
        } else {
            Some(stored_json(instance.proof_seals())?)
        },
    })
}

/// The value of one instance of `name`, in the column its shape names.
fn stored_value(name: &AttributeName, family: &FamilyValue, keys: &NameKeys) -> Result<StoredValue, FlattenError> {
    match family {
        FamilyValue::Property { values, .. } => match scalar_literal(values) {
            Some(literal) => literal_value(name, literal, keys),
            None => object(&StructuredValue::Values(values.iter().map(ExpandedValue::to_fragment).collect())),
        },
        FamilyValue::GeoProperty { geometry, .. } => Ok(match geometry {
            GeoValue::Geometry(geometry) => StoredValue::Geometry {
                form: geometry.form().clone(), // clone: the row owns the positions it binds
                members: geometry.members().map(stored_json).transpose()?,
            },
            GeoValue::Encoded(encoded) => StoredValue::String {
                text: encoded.as_str().to_owned(),
                annotation: LiteralAnnotation::Plain,
            },
        }),
        FamilyValue::Relationship { objects, .. } => match objects.as_slice() {
            [object] => Ok(StoredValue::Uri(object.clone())),              // clone: the row owns the object it binds
            _ => self::object(&StructuredValue::Objects(objects.clone())), // clone: the codec writes the objects it owns
        },
        FamilyValue::LanguageProperty { entries, .. } => object(&StructuredValue::LanguageMap(entries.to_fragment())),
        FamilyValue::VocabProperty { vocab, .. } => match vocab.as_slice() {
            [term] => Ok(StoredValue::String {
                text: term.as_str().to_owned(),
                annotation: LiteralAnnotation::Plain,
            }),
            _ => object(&StructuredValue::Vocabs(vocab.clone())), // clone: the codec writes the terms it owns
        },
        FamilyValue::JsonProperty { json, .. } => match json.as_slice() {
            [literal] => object(&StructuredValue::Json(literal.clone())), // clone: the codec writes the literal it owns
            _ => Err(FlattenError::SeveralJsonLiterals(name.clone())),
        },
        FamilyValue::ListProperty { items, .. } => object(&StructuredValue::List(items.iter().map(ExpandedValue::to_fragment).collect())),
        FamilyValue::ListRelationship { objects, .. } => objects
            .iter()
            .map(|object| match object {
                // clone: the codec writes the targets it owns
                ListObject::Wrapped(uri) => Ok(uri.clone()),
                // clone: the refusal owns the name it reports.
                ListObject::Bare(_) | ListObject::Literal(_) => Err(FlattenError::UncommittedSpelling(name.clone())),
            })
            .collect::<Result<Vec<_>, _>>()
            .and_then(|targets| object(&StructuredValue::ObjectList(targets))),
    }
}

/// The literal a Property's values hold as one scalar: a single literal with
/// no direction. Any other value is held whole in `value_object`.
#[must_use]
pub const fn scalar_literal(values: &[ExpandedValue]) -> Option<&Literal> {
    match values {
        [ExpandedValue::Literal(literal)] if literal.direction().is_none() => Some(literal),
        [ExpandedValue::Literal(_) | ExpandedValue::Json(_) | ExpandedValue::Node(_) | ExpandedValue::List(_)] | [] | [_, _, ..] => None,
    }
}

/// The value of a Property holding one literal with no direction.
fn literal_value(name: &AttributeName, literal: &Literal, keys: &NameKeys) -> Result<StoredValue, FlattenError> {
    let datatype = literal
        .datatype()
        // clone: the refusal owns the name it reports.
        .map(|datatype| keys.type_name(datatype).ok_or_else(|| FlattenError::UnresolvedType(datatype.clone())))
        .transpose()?;
    Ok(match literal.value() {
        LiteralValue::Number(number) => StoredValue::Number {
            number: number.clone(), // clone: the row owns its exact number
            datatype,
        },
        LiteralValue::Boolean(flag) => StoredValue::Boolean { flag: *flag, datatype },
        LiteralValue::String(text) => match TimeValue::admitted(literal.datatype().map(NgsiLdReference::as_str), text) {
            Some(Ok(TimeValue::DateTime(instant))) => {
                if sub_second(instant.get()) != text.as_str() {
                    // clone: the refusal owns the name it reports.
                    return Err(FlattenError::UncommittedSpelling(name.clone()));
                }
                StoredValue::DateTime {
                    instant: instant.get(),
                    datatype,
                }
            }
            Some(Ok(TimeValue::Date(_) | TimeValue::Time(_)) | Err(_)) | None => StoredValue::String {
                text: text.to_string(),
                annotation: string_annotation(literal, datatype),
            },
        },
    })
}

/// What a string literal carries beside its text.
fn string_annotation(literal: &Literal, datatype: Option<EntityTypeNameKey>) -> LiteralAnnotation {
    match (datatype, literal.language()) {
        (Some(datatype), _) => LiteralAnnotation::Datatype(datatype),
        // clone: the row owns the tag it binds
        (None, Some(language)) => LiteralAnnotation::Language(language.clone()),
        (None, None) => LiteralAnnotation::Plain,
    }
}

/// A structured value in `value_object`.
fn object(value: &StructuredValue) -> Result<StoredValue, FlattenError> {
    Ok(StoredValue::Object(StoredJson::new(value.to_stored_json()?)))
}

/// A piece in the JSON its codec writes.
fn stored_json(piece: &impl Serialize) -> Result<StoredJson, FlattenError> {
    Ok(StoredJson::new(to_raw_value(piece)?))
}
