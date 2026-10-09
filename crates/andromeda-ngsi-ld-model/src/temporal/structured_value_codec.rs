//! The JSON a history row stores a structured value as, both ways.
//!
//! `attribute_history.value_object` and `current_attributes.value_object` are
//! read inside PostgreSQL: the aggregation tells an array from an object
//! with `jsonb_typeof` and counts `jsonb_array_length`, `distinct` and
//! `aggrGroupBy` compare `value_object::text`, and the temporal `q` rebuilds an
//! expanded instance around it. The stored JSON is therefore a fixed contract,
//! one per family:
//!
//! | family | stored JSON |
//! |---|---|
//! | Property, one structured value | the expanded value: a node object, `{"@list": [..]}`, or `{"@value": .., "@type": "@json"}` |
//! | Property, several values | an array, a plain scalar value object as its bare scalar, any other value (a typed or tagged scalar included) as its expanded form |
//! | Relationship, several targets | `["urn:a", "urn:b"]` |
//! | `VocabProperty`, several terms | `["iri1", "iri2"]` |
//! | `LanguageProperty` | the expanded language map, `[{"@value": .., "@language": ..}]` |
//! | `JsonProperty` | the JSON literal itself |
//! | `ListProperty` | the `@list` body, `[{"@value": 1}, ..]` |
//! | `ListRelationship` | `["urn:a", "urn:b"]` |
//!
//! Reading one back needs the family, because the same JSON (an array of
//! strings) means a different value in different families. The expanded
//! pieces are decoded by the canonical codec of the model, through the member
//! of an instance that carries them.

use crate::temporal::structured_value::StructuredValue;
use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError};
use andromeda_ngsi_ld_entity::{
    document::{
        instance_document::InstanceDocument,
        value_carrier::ValueCarrier,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::ExpandedValue,
    literal::LiteralValue,
    non_empty::{EmptyCollection, NonEmpty},
    refusal::EntityRefusal,
    value_mode::Fragment,
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    reference::{NgsiLdReference, NgsiLdReferenceError},
    uri::{Uri, UriParseError},
};
use serde::{Serialize, Serializer};
use serde_json::value::{RawValue, to_raw_value};
use thiserror::Error;

/// A stored structured value that does not read as the value of its family.
#[derive(Debug, Error)]
pub enum StoredValueError {
    /// The stored text is not JSON, or not JSON of the shape the family stores.
    #[error("the stored structured value is not shaped as its family stores it: {0}")]
    Json(#[from] serde_json::Error),
    /// The stored JSON literal of a `JsonProperty` is not one the broker
    /// admits.
    #[error("the stored JSON literal is not admissible: {0}")]
    Literal(#[from] JsonLiteralError),
    /// The expanded pieces do not read as an expanded value.
    #[error(transparent)]
    Refused(#[from] EntityRefusal),
    /// The pieces read as the value of another member than the family's.
    #[error("the stored structured value reads as the '{}' member rather than the family's", .carrier.iri())]
    Carrier {
        /// The member the pieces read as.
        carrier: ValueCarrier,
    },
    /// The pieces read as the deletion of the value, or as no value at all.
    #[error("the stored structured value holds no value")]
    NoValue,
    /// A stored target is not a URI.
    #[error("a stored target is not a URI: {0}")]
    Target(#[from] UriParseError),
    /// A stored vocabulary term is not a name.
    #[error("a stored vocabulary term is not a name: {0}")]
    Term(#[from] NgsiLdReferenceError),
    /// The family holds at least one element and the stored array holds none.
    #[error("the stored structured value holds no element")]
    Empty(#[from] EmptyCollection),
}

/// Written by hand because the stored JSON is the per-family contract the
/// module documentation lays out, which no derive spells: a value is written
/// bare in one family and expanded in another.
impl Serialize for StructuredValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            StructuredValue::Values(values) => match values.as_slice() {
                [single] => single.serialize(serializer),
                several => serializer.collect_seq(several.iter().map(ArrayElement)),
            },
            StructuredValue::Objects(targets) => serializer.collect_seq(targets.iter().map(Uri::as_str)),
            StructuredValue::Vocabs(terms) => serializer.collect_seq(terms.iter().map(NgsiLdReference::as_str)),
            StructuredValue::LanguageMap(entries) => entries.serialize(serializer),
            StructuredValue::Json(literal) => literal.serialize(serializer),
            StructuredValue::List(items) => serializer.collect_seq(items),
            StructuredValue::ObjectList(targets) => serializer.collect_seq(targets.iter().map(Uri::as_str)),
        }
    }
}

/// One element of an array-valued Property as the array stores it: a plain
/// scalar value object as its bare scalar, so an array of scalars is the plain
/// JSON array it was written as, and any other value, a typed or tagged scalar
/// included, in its expanded form. Clause 4.5.7 records each instance as the
/// Property it was, so a datatype or a language written on an element is part
/// of the value and survives the history.
struct ArrayElement<'a>(&'a ExpandedValue<Fragment>);

impl Serialize for ArrayElement<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            ExpandedValue::Literal(literal) if literal.datatype().is_none() && literal.language().is_none() && literal.direction().is_none() => {
                match literal.value() {
                    LiteralValue::Number(number) => number.serialize(serializer),
                    LiteralValue::String(text) => serializer.serialize_str(text),
                    LiteralValue::Boolean(flag) => serializer.serialize_bool(*flag),
                }
            }
            expanded @ (ExpandedValue::Literal(_) | ExpandedValue::Json(_) | ExpandedValue::Node(_) | ExpandedValue::List(_)) => expanded.serialize(serializer),
        }
    }
}

impl StructuredValue {
    /// The JSON a history row stores the value as.
    ///
    /// # Errors
    ///
    /// Returns the serializer's failure, which a value of the model never
    /// produces.
    pub fn to_stored_json(&self) -> Result<Box<RawValue>, serde_json::Error> {
        to_raw_value(self)
    }

    /// Read the structured value a row of `attribute`, recorded in `family`,
    /// stores as `json`.
    ///
    /// # Errors
    ///
    /// Returns [`StoredValueError`] when `json` is not the stored JSON of a
    /// value of `family`.
    pub fn from_stored(family: AttributeType, attribute: &AttributeName, json: &str) -> Result<StructuredValue, StoredValueError> {
        match family {
            AttributeType::Property | AttributeType::GeoProperty => property_values(attribute, json),
            AttributeType::Relationship => Ok(StructuredValue::Objects(NonEmpty::try_from_vec(uris(json)?)?)),
            AttributeType::VocabProperty => Ok(StructuredValue::Vocabs(NonEmpty::try_from_vec(terms(json)?)?)),
            AttributeType::LanguageProperty => match carried(attribute, ValueCarrier::LanguageMap, json)? {
                WrittenValue::LanguageMap(entries) => Ok(StructuredValue::LanguageMap(entries)),
                other @ (WrittenValue::Value(_)
                | WrittenValue::Geometry(_)
                | WrittenValue::Object(_)
                | WrittenValue::Vocab(_)
                | WrittenValue::Json(_)
                | WrittenValue::ValueList(_)
                | WrittenValue::ObjectList(_)) => Err(StoredValueError::Carrier { carrier: other.carrier() }),
            },
            AttributeType::JsonProperty => Ok(StructuredValue::Json(JsonLiteral::admit(json)?)),
            AttributeType::ListProperty => match carried(attribute, ValueCarrier::ValueList, &format!("[{{\"@list\":{json}}}]"))? {
                WrittenValue::ValueList(items) => Ok(StructuredValue::List(items)),
                other @ (WrittenValue::Value(_)
                | WrittenValue::Geometry(_)
                | WrittenValue::Object(_)
                | WrittenValue::LanguageMap(_)
                | WrittenValue::Vocab(_)
                | WrittenValue::Json(_)
                | WrittenValue::ObjectList(_)) => Err(StoredValueError::Carrier { carrier: other.carrier() }),
            },
            AttributeType::ListRelationship => Ok(StructuredValue::ObjectList(uris(json)?)),
        }
    }
}

/// Read the stored value of a Property: one expanded value, or an array of
/// values whose scalars are stored bare.
fn property_values(attribute: &AttributeName, json: &str) -> Result<StructuredValue, StoredValueError> {
    let elements: Vec<&RawValue> = if json.trim_start().starts_with('[') {
        serde_json::from_str(json)?
    } else {
        vec![serde_json::from_str(json)?]
    };
    let mut member = String::with_capacity(json.len() + elements.len() * 12 + 2);
    member.push('[');
    for (position, element) in elements.iter().enumerate() {
        if position > 0 {
            member.push(',');
        }
        let text = element.get();
        if text.trim_start().starts_with('{') {
            member.push_str(text);
        } else {
            member.push_str("{\"@value\":");
            member.push_str(text);
            member.push('}');
        }
    }
    member.push(']');
    match carried(attribute, ValueCarrier::Value, &member)? {
        WrittenValue::Value(values) => Ok(StructuredValue::Values(values)),
        other @ (WrittenValue::Geometry(_)
        | WrittenValue::Object(_)
        | WrittenValue::LanguageMap(_)
        | WrittenValue::Vocab(_)
        | WrittenValue::Json(_)
        | WrittenValue::ValueList(_)
        | WrittenValue::ObjectList(_)) => Err(StoredValueError::Carrier { carrier: other.carrier() }),
    }
}

/// Decode `member`, the expanded text of one value member, as the value the
/// member `carrier` of an instance of `attribute` carries.
fn carried(attribute: &AttributeName, carrier: ValueCarrier, member: &str) -> Result<WrittenValue, StoredValueError> {
    let key = carrier.iri();
    let mut instance = String::with_capacity(member.len() + key.as_str().len() + 6);
    instance.push_str("{\"");
    instance.push_str(key.as_str());
    instance.push_str("\":");
    instance.push_str(member);
    instance.push('}');
    match InstanceDocument::from_slice(attribute, instance.as_bytes())?.into_parts().value {
        ValueSlot::Set(value) => Ok(value),
        ValueSlot::Absent | ValueSlot::Remove(_) => Err(StoredValueError::NoValue),
    }
}

/// Read a stored array of target URIs.
fn uris(json: &str) -> Result<Vec<Uri>, StoredValueError> {
    serde_json::from_str::<Vec<String>>(json)?
        .into_iter()
        .map(|target| target.parse::<Uri>().map_err(StoredValueError::from))
        .collect()
}

/// Read a stored array of vocabulary terms.
fn terms(json: &str) -> Result<Vec<NgsiLdReference>, StoredValueError> {
    serde_json::from_str::<Vec<String>>(json)?
        .into_iter()
        .map(|term| term.parse::<NgsiLdReference>().map_err(StoredValueError::from))
        .collect()
}
