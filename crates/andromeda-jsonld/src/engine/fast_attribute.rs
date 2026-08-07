//! Expanding one NGSI-LD attribute without the JSON-LD processor.
//!
//! Six of the eight attribute types clause 5.2.4 defines are handled here.
//! `ListProperty` and `ListRelationship` are refused deliberately: their
//! `@list` semantics are not what the other six share, and approximating them
//! would be silent. Everything else this walker is not certain of answers
//! `None` so the caller falls back to the processor.
//!
//! Which member of an attribute holds its value, and what that member expands
//! to, are one eight-way table each. Both are
//! [`andromeda_ngsi_ld_lexicon::attribute_type::AttributeType`]'s
//! rather than this module's, so a ninth attribute type cannot be added to the
//! specification crate and forgotten here.

use crate::engine::{
    expanded_key,
    term_map::{BatchTermMap, TermCoercion},
};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_vocabulary::compact_to_expanded;
use serde_json::{Map, Value};
use std::str::FromStr;

/// Expand one attribute slot: a single instance, several, or a bare scalar.
///
/// A bare scalar is the concise form clause 5.2.4 wraps as an implicit
/// `Property`; that wrap happens here rather than in a separate pass over the
/// document.
pub(crate) fn expand_attribute(value: &Value, term_map: &BatchTermMap) -> Option<Value> {
    match value {
        Value::Object(instance) => Some(Value::Array(vec![expand_instance(instance, term_map)?])),
        Value::Array(instances) => {
            if instances.is_empty() {
                return None;
            }
            let mut expanded = Vec::with_capacity(instances.len());
            for instance in instances {
                expanded.push(expand_instance(instance.as_object()?, term_map)?);
            }
            Some(Value::Array(expanded))
        }
        Value::String(_) | Value::Number(_) | Value::Bool(_) => Some(implicit_property(value)),
        Value::Null => None,
    }
}

/// The attribute type an instance names, if this walker expands that one.
fn supported_kind(instance: &Map<String, Value>) -> Option<AttributeType> {
    let named = AttributeType::from_str(instance.get("type")?.as_str()?).ok()?;
    match named {
        AttributeType::Property
        | AttributeType::Relationship
        | AttributeType::GeoProperty
        | AttributeType::LanguageProperty
        | AttributeType::VocabProperty
        | AttributeType::JsonProperty => Some(named),
        AttributeType::ListProperty | AttributeType::ListRelationship => None,
    }
}

/// The expanded form of a concise scalar, as an implicit `Property`.
fn implicit_property(scalar: &Value) -> Value {
    let mut value_node = Map::with_capacity(1);
    value_node.insert("@value".to_owned(), scalar.clone()); // clone: the source document is borrowed and stays valid

    let mut instance = Map::with_capacity(2);
    instance.insert(
        "@type".to_owned(),
        Value::Array(vec![Value::String(AttributeType::Property.iri_str().to_owned())]),
    );
    instance.insert(
        AttributeType::Property.value_key_str().to_owned(),
        Value::Array(vec![Value::Object(value_node)]),
    );

    Value::Array(vec![Value::Object(instance)])
}

/// Expand one attribute instance, without the enclosing array.
fn expand_instance(instance: &Map<String, Value>, term_map: &BatchTermMap) -> Option<Value> {
    let kind = supported_kind(instance)?;

    let mut expanded = Map::with_capacity(instance.len());
    expanded.insert("@type".to_owned(), Value::Array(vec![Value::String(kind.iri_str().to_owned())]));
    expanded.insert(kind.value_key_str().to_owned(), expand_value(instance, kind, term_map)?);

    for (key, value) in instance {
        if key == "type" || key == kind.compact_value_key() {
            continue;
        }
        let (iri, node) = expand_metadata(key, value, term_map)?;
        expanded.insert(iri, node);
    }

    Some(Value::Object(expanded))
}

/// Expand the value member of one attribute instance.
fn expand_value(instance: &Map<String, Value>, kind: AttributeType, term_map: &BatchTermMap) -> Option<Value> {
    let value = instance.get(kind.compact_value_key())?;
    match kind {
        AttributeType::Property => scalar_value(value),
        AttributeType::Relationship => Some(Value::Array(vec![id_node(value.as_str()?)])),
        AttributeType::GeoProperty => Some(Value::Array(vec![expand_geometry(value.as_object()?)?])),
        AttributeType::LanguageProperty => expand_language_map(value.as_object()?),
        AttributeType::VocabProperty => expand_vocab(value, term_map),
        AttributeType::JsonProperty => Some(json_literal(value)),
        // Refused by `supported_kind` before this function is reached.
        AttributeType::ListProperty | AttributeType::ListRelationship => None,
    }
}

/// A `@json` literal node: the value kept verbatim, tagged `@type: @json`.
///
/// The shape a `JsonProperty` value and a `@type: @json` coerced metadata
/// member both expand to (clause 4.5.5 / 5.2.4).
fn json_literal(value: &Value) -> Value {
    let mut node = Map::with_capacity(2);
    node.insert("@type".to_owned(), Value::String("@json".to_owned()));
    node.insert("@value".to_owned(), value.clone()); // clone: the source document is borrowed and stays valid
    Value::Array(vec![Value::Object(node)])
}

/// The expanded form of a `Property` value when it is scalar or an array of
/// scalars.
///
/// Object-valued values still bail because expanding them depends on term
/// definitions this walker does not apply below the attribute level.
fn scalar_value(value: &Value) -> Option<Value> {
    match value {
        Value::String(_) | Value::Number(_) | Value::Bool(_) => {
            let mut node = Map::with_capacity(1);
            node.insert("@value".to_owned(), value.clone()); // clone: the source document is borrowed and stays valid
            Some(Value::Array(vec![Value::Object(node)]))
        }
        Value::Array(values) => {
            let mut expanded = Vec::with_capacity(values.len());
            for scalar in values {
                match scalar {
                    Value::String(_) | Value::Number(_) | Value::Bool(_) => {
                        let mut node = Map::with_capacity(1);
                        node.insert("@value".to_owned(), scalar.clone()); // clone: the source document is borrowed and stays valid
                        expanded.push(Value::Object(node));
                    }
                    Value::Null => {}
                    Value::Object(_) | Value::Array(_) => return None,
                }
            }
            Some(Value::Array(expanded))
        }
        Value::Null | Value::Object(_) => None,
    }
}

/// A node naming an identifier.
fn id_node(id: &str) -> Value {
    let mut node = Map::with_capacity(1);
    node.insert("@id".to_owned(), Value::String(id.to_owned()));
    Value::Object(node)
}

/// Expand a GeoJSON geometry into the core context's expanded form.
fn expand_geometry(geometry: &Map<String, Value>) -> Option<Value> {
    let type_iri = compact_to_expanded(geometry.get("type")?.as_str()?)?;

    let mut expanded = Map::with_capacity(geometry.len());
    expanded.insert("@type".to_owned(), Value::Array(vec![Value::String(type_iri.to_owned())]));

    for (key, value) in geometry {
        match key.as_str() {
            "type" => {}
            "coordinates" => {
                expanded.insert(expanded_key::COORDINATES.clone(), Value::Array(vec![expand_coordinates(value)?]));
            }
            "bbox" => {
                expanded.insert(expanded_key::BBOX.clone(), Value::Array(vec![expand_coordinates(value)?]));
            }
            _other => return None,
        }
    }
    Some(Value::Object(expanded))
}

/// Expand a GeoJSON coordinate structure into nested `@list` wrappers.
fn expand_coordinates(value: &Value) -> Option<Value> {
    match value {
        Value::Array(items) => {
            let mut list = Vec::with_capacity(items.len());
            for item in items {
                list.push(expand_coordinates(item)?);
            }
            let mut node = Map::with_capacity(1);
            node.insert("@list".to_owned(), Value::Array(list));
            Some(Value::Object(node))
        }
        Value::Number(_) => {
            let mut node = Map::with_capacity(1);
            node.insert("@value".to_owned(), value.clone()); // clone: the source document is borrowed and stays valid
            Some(Value::Object(node))
        }
        Value::Null | Value::Bool(_) | Value::String(_) | Value::Object(_) => None,
    }
}

/// Expand a language map into one `@language`-tagged node per entry.
fn expand_language_map(map: &Map<String, Value>) -> Option<Value> {
    let mut expanded = Vec::with_capacity(map.len());
    for (language, text) in map {
        let mut node = Map::with_capacity(2);
        // A language map's `@none` key means "no language tag": the processor
        // emits a bare `@value` node, so tagging it `@language: @none` here would
        // diverge from the fallback path (JSON-LD 1.1 clause 9.5).
        if language != "@none" {
            node.insert("@language".to_owned(), Value::String(language.clone())); // clone: the source document is borrowed and stays valid
        }
        node.insert("@value".to_owned(), Value::String(text.as_str()?.to_owned()));
        expanded.push(Value::Object(node));
    }
    Some(Value::Array(expanded))
}

/// Expand a `VocabProperty` value, which resolves through the term table.
fn expand_vocab(value: &Value, term_map: &BatchTermMap) -> Option<Value> {
    match value {
        Value::String(term) => Some(Value::Array(vec![id_node(&term_map.expand_iri(term))])),
        Value::Array(terms) => {
            let mut expanded = Vec::with_capacity(terms.len());
            for term in terms {
                expanded.push(id_node(&term_map.expand_iri(term.as_str()?)));
            }
            Some(Value::Array(expanded))
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => None,
    }
}

/// Expand an attribute-level metadata member: `observedAt`, `datasetId`,
/// `unitCode`, or a nested sub-attribute.
fn expand_metadata(key: &str, value: &Value, term_map: &BatchTermMap) -> Option<(String, Value)> {
    // A term with a scoped `@context` changes what its members mean, which
    // this walker does not unfold.
    if term_map.term_has_scoped_context(key) {
        return None;
    }
    // A `@container` reshapes the value in a way the flat walker cannot model,
    // so a metadata member carrying one is refused rather than mis-expanded.
    if term_map.term_has_container(key) {
        return None;
    }
    let iri = term_map.expand_iri(key).into_owned();
    let coercion = term_map.coercion(key);

    // A `@type: @json` term keeps its value verbatim, whatever its JSON shape,
    // so it is handled before the per-shape dispatch below.
    if matches!(coercion, Some(TermCoercion::Json)) {
        return Some((iri, json_literal(value)));
    }

    match value {
        Value::String(text) => Some((iri, coerced_scalar(text, coercion, term_map))),
        // A coerced non-string has no expansion this walker can state, since
        // the coercion says how to read a lexical form.
        Value::Number(_) | Value::Bool(_) => match coercion {
            Some(_declared) => None,
            None => {
                let mut node = Map::with_capacity(1);
                node.insert("@value".to_owned(), value.clone()); // clone: the source document is borrowed and stays valid
                Some((iri, Value::Array(vec![Value::Object(node)])))
            }
        },
        // A nested sub-attribute is an attribute; the same walker applies.
        Value::Object(sub) => Some((iri, Value::Array(vec![expand_sub_attribute(sub, term_map)?]))),
        Value::Array(instances) => {
            let mut expanded = Vec::with_capacity(instances.len());
            for instance in instances {
                expanded.push(expand_sub_attribute(instance.as_object()?, term_map)?);
            }
            Some((iri, Value::Array(expanded)))
        }
        Value::Null => None,
    }
}

/// Expand a nested attribute instance, refusing anything that is not one.
fn expand_sub_attribute(sub: &Map<String, Value>, term_map: &BatchTermMap) -> Option<Value> {
    supported_kind(sub)?;
    expand_instance(sub, term_map)
}

/// The expanded form of a metadata string under the coercion its term
/// declares.
fn coerced_scalar(text: &str, coercion: Option<&TermCoercion>, term_map: &BatchTermMap) -> Value {
    let node = match coercion {
        Some(TermCoercion::DateTime) => typed_value(&expanded_key::DATE_TIME, text),
        Some(TermCoercion::Id) => return Value::Array(vec![id_node(text)]),
        // A `@type: @vocab` value is a term, expanded against the active
        // `@vocab` exactly as a VocabProperty value is (clause 4.5.5).
        Some(TermCoercion::Vocab) => return Value::Array(vec![id_node(&term_map.expand_iri(text))]),
        // A `@type: @json` string is kept verbatim as a JSON literal; also
        // caught in `expand_metadata` before dispatch, so this arm is a
        // completeness guard rather than the live path.
        Some(TermCoercion::Json) => return json_literal(&Value::String(text.to_owned())),
        Some(TermCoercion::TypeIri(declared)) => typed_value(&term_map.expand_iri(declared), text),
        None => {
            let mut node = Map::with_capacity(1);
            node.insert("@value".to_owned(), Value::String(text.to_owned()));
            node
        }
    };
    Value::Array(vec![Value::Object(node)])
}

/// A `{"@type": …, "@value": …}` node.
fn typed_value(type_iri: &str, text: &str) -> Map<String, Value> {
    let mut node = Map::with_capacity(2);
    node.insert("@type".to_owned(), Value::String(type_iri.to_owned()));
    node.insert("@value".to_owned(), Value::String(text.to_owned()));
    node
}
