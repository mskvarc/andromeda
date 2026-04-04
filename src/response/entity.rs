//! Presentation / response-formatting logic for entity queries:
//! language negotiation, inline joins, keyValues simplification.

use crate::{
    domain::{
        attribute::{Attribute, AttributeValue, LanguageProperty},
        entity::Entity,
        serde_helpers,
        uri::Uri,
    },
    error::AndromedaError,
    query::lang::{LanguageFilter, LanguageFilterError, LanguageRange},
};
use chrono::{DateTime, SecondsFormat, Utc};
use indexmap::IndexMap;
use language_tags::LanguageTag;
use serde::Serialize;
use serde_json::{Map, Value};
use std::{collections::HashMap, ops::Deref, str::FromStr};

/// Render an entity to a response value, applying the optional NGSI-LD `lang`
/// filter only at the presentation boundary.
pub fn render_entity(entity: &Entity, language_filter: Option<&LanguageFilter>) -> Result<Value, AndromedaError> {
    if language_filter.is_none() {
        return serialize_value(entity);
    }

    let mut map = Map::new();
    map.insert("id".to_string(), serialize_value(&entity.id)?);
    map.insert("type".to_string(), serialize_value(&entity.type_)?);

    if let Some(scope) = &entity.scope {
        map.insert("scope".to_string(), serialize_value(scope)?);
    }
    if let Some(expires_at) = &entity.expires_at {
        map.insert("expiresAt".to_string(), Value::String(expires_at.to_rfc3339_opts(SecondsFormat::Secs, true)));
    }
    if let Some(location) = &entity.location {
        map.insert("location".to_string(), render_attribute_value(location, language_filter)?);
    }
    if let Some(observation_space) = &entity.observation_space {
        map.insert("observationSpace".to_string(), render_attribute_value(observation_space, language_filter)?);
    }
    if let Some(operation_space) = &entity.operation_space {
        map.insert("operationSpace".to_string(), render_attribute_value(operation_space, language_filter)?);
    }
    if let Some(created_at) = &entity.created_at {
        map.insert("createdAt".to_string(), Value::String(created_at.to_rfc3339_opts(SecondsFormat::Secs, true)));
    }
    if let Some(modified_at) = &entity.modified_at {
        map.insert("modifiedAt".to_string(), Value::String(modified_at.to_rfc3339_opts(SecondsFormat::Secs, true)));
    }
    if let Some(deleted_at) = &entity.deleted_at {
        map.insert("deletedAt".to_string(), Value::String(deleted_at.to_rfc3339_opts(SecondsFormat::Secs, true)));
    }

    for (key, attr) in &entity.attrs {
        map.insert(key.clone(), render_attribute_value(attr, language_filter)?);
    }

    Ok(Value::Object(map))
}

/// Convert an entity to simplified/keyValues form while preserving the legacy
/// serialization path whenever no `lang` negotiation is active.
pub fn simplify_entity(entity: &Entity, language_filter: Option<&LanguageFilter>) -> Result<Value, AndromedaError> {
    match language_filter {
        Some(filter) => render_entity(entity, Some(filter)).map(|value| simplify_value(&value)),
        None => Ok(entity.simplify()),
    }
}

fn render_attribute_value(attr: &AttributeValue, language_filter: Option<&LanguageFilter>) -> Result<Value, AndromedaError> {
    match attr {
        AttributeValue::Single(attribute) => render_attribute(attribute.deref(), language_filter),
        AttributeValue::Multiple(attributes) => attributes
            .iter()
            .map(|attribute| render_attribute(attribute, language_filter))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        AttributeValue::Raw(value) => Ok(value.clone()),
    }
}

fn render_attribute(attribute: &Attribute, language_filter: Option<&LanguageFilter>) -> Result<Value, AndromedaError> {
    match attribute {
        Attribute::LanguageProperty(language_property) => render_language_property(language_property, language_filter),
        _ => serialize_value(attribute),
    }
}

fn render_language_property(language_property: &LanguageProperty, language_filter: Option<&LanguageFilter>) -> Result<Value, AndromedaError> {
    if let Some(filter) = language_filter
        && let Some(negotiated) = negotiate_language_property(language_property, filter)?
    {
        return serialize_value(&negotiated);
    }

    // Clone required: Attribute's #[serde(tag = "type")] adds the "type" discriminant during serialization.
    // Serializing LanguageProperty directly would omit the type tag.
    serialize_value(&Attribute::LanguageProperty(language_property.clone()))
}

fn negotiate_language_property<'a>(language_property: &'a LanguageProperty, filter: &LanguageFilter) -> Result<Option<NegotiatedProperty<'a>>, AndromedaError> {
    let available = language_entries(&language_property.language_map)?;

    for preference in filter.ordered() {
        match preference.range() {
            LanguageRange::Wildcard => {
                if let Some(entry) = available.iter().find(|entry| matches!(entry.range, AvailableLanguageRange::Tag(_))) {
                    return Ok(Some(NegotiatedProperty::from_language_property(language_property, entry)));
                }
                if let Some(entry) = available.iter().find(|entry| matches!(entry.range, AvailableLanguageRange::Default)) {
                    return Ok(Some(NegotiatedProperty::from_language_property(language_property, entry)));
                }
            }
            LanguageRange::Tag(requested) => {
                if let Some(entry) = available.iter().find(|entry| entry.exact_matches(requested)) {
                    return Ok(Some(NegotiatedProperty::from_language_property(language_property, entry)));
                }
                if requested.is_language_range()
                    && let Some(entry) = available.iter().find(|entry| entry.range_matches(requested))
                {
                    return Ok(Some(NegotiatedProperty::from_language_property(language_property, entry)));
                }
            }
        }
    }

    Ok(None)
}

fn language_entries(language_map: &Value) -> Result<Vec<AvailableLanguage<'_>>, AndromedaError> {
    let map = language_map
        .as_object()
        .ok_or_else(|| AndromedaError::internal("LanguageProperty languageMap must be a JSON object"))?;

    map.iter()
        .filter_map(|(tag, value)| extract_language_value(value).map(|val| (tag, val)))
        .map(|(tag, value)| AvailableLanguage::new(tag, value))
        .collect()
}

/// Extract a language map value, preserving both strings and non-empty arrays.
///
/// Per NGSI-LD spec clause 5.7.2.5, language map values can be arrays of strings.
fn extract_language_value(value: &Value) -> Option<&Value> {
    match value {
        Value::String(_) => Some(value),
        Value::Array(items) if !items.is_empty() => Some(value),
        Value::Array(_) | Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => None,
    }
}

fn serialize_value<T>(value: &T) -> Result<Value, AndromedaError>
where
    T: Serialize,
{
    serde_json::to_value(value).map_err(|error| AndromedaError::internal("Failed to serialize response value").with_source(error))
}

#[derive(Debug)]
struct AvailableLanguage<'a> {
    range: AvailableLanguageRange,
    value: &'a Value,
}

impl<'a> AvailableLanguage<'a> {
    fn new(tag: &str, value: &'a Value) -> Result<AvailableLanguage<'a>, AndromedaError> {
        let range = if tag == "@none" {
            AvailableLanguageRange::Default
        } else {
            let parsed = LanguageTag::from_str(tag).map_err(|error| {
                AndromedaError::bad_request_data(
                    LanguageFilterError::InvalidLanguageTag {
                        value: tag.to_string(),
                        source: error,
                    }
                    .to_string(),
                )
            })?;
            let canonical = parsed.canonicalize().map_err(|error| {
                AndromedaError::bad_request_data(
                    LanguageFilterError::InvalidStoredLanguageTag {
                        value: tag.to_string(),
                        source: error,
                    }
                    .to_string(),
                )
            })?;
            AvailableLanguageRange::Tag(canonical)
        };

        Ok(AvailableLanguage { range, value })
    }

    fn exact_matches(&self, requested: &LanguageTag) -> bool {
        match &self.range {
            AvailableLanguageRange::Tag(tag) => tag == requested,
            AvailableLanguageRange::Default => false,
        }
    }

    fn range_matches(&self, requested: &LanguageTag) -> bool {
        match &self.range {
            AvailableLanguageRange::Tag(tag) => requested.matches(tag),
            AvailableLanguageRange::Default => false,
        }
    }
}

#[derive(Debug, Clone)]
enum AvailableLanguageRange {
    Default,
    Tag(LanguageTag),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NegotiatedProperty<'a> {
    #[serde(rename = "type")]
    type_: &'static str,
    value: &'a Value,
    lang: String,
    #[serde(skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    observed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dataset_id: &'a Option<Uri>,
    #[serde(flatten)]
    sub_attributes: &'a IndexMap<String, AttributeValue>,
}

impl<'a> NegotiatedProperty<'a> {
    fn from_language_property(language_property: &'a LanguageProperty, selected: &AvailableLanguage<'a>) -> NegotiatedProperty<'a> {
        NegotiatedProperty {
            type_: "Property",
            value: selected.value,
            lang: match &selected.range {
                AvailableLanguageRange::Default => "@none".to_string(),
                AvailableLanguageRange::Tag(tag) => tag.to_string(),
            },
            observed_at: language_property.observed_at,
            dataset_id: &language_property.dataset_id,
            sub_attributes: &language_property.sub_attributes,
        }
    }
}

/// Apply `join=inline` expansion to an already-rendered entity response.
pub fn apply_inline_join(entity: &mut Value, linked: &HashMap<String, Entity>, language_filter: Option<&LanguageFilter>) -> Result<(), AndromedaError> {
    let Some(obj) = entity.as_object_mut() else { return Ok(()) };
    for attr_val in obj.values_mut() {
        match attr_val {
            Value::Object(attr_obj) => {
                let attr_type = attr_obj.get("type").and_then(Value::as_str).map(ToOwned::to_owned);
                if attr_type.as_deref() == Some("Relationship") {
                    if let Some(object_uri) = attr_obj.get("object").and_then(Value::as_str).map(ToOwned::to_owned)
                        && let Some(linked_entity) = linked.get(&object_uri)
                    {
                        let mut linked_val = render_entity(linked_entity, language_filter)?;
                        apply_inline_join(&mut linked_val, linked, language_filter)?;
                        attr_obj.insert("entity".to_string(), linked_val);
                    }
                } else if attr_type.as_deref() == Some("ListRelationship")
                    && let Some(object_list) = attr_obj.get("objectList").and_then(Value::as_array)
                {
                    let entity_list = object_list
                        .iter()
                        .filter_map(Value::as_str)
                        .filter_map(|uri| linked.get(uri))
                        .map(|linked_entity| {
                            let mut linked_val = render_entity(linked_entity, language_filter)?;
                            apply_inline_join(&mut linked_val, linked, language_filter)?;
                            Ok(linked_val)
                        })
                        .collect::<Result<Vec<_>, AndromedaError>>()?;
                    if !entity_list.is_empty() {
                        attr_obj.insert("entityList".to_string(), Value::Array(entity_list));
                    }
                }
            }
            Value::Array(items) => {
                for item in items.iter_mut() {
                    if let Value::Object(attr_obj) = item {
                        let attr_type = attr_obj.get("type").and_then(Value::as_str).map(ToOwned::to_owned);
                        if attr_type.as_deref() == Some("Relationship") {
                            if let Some(object_uri) = attr_obj.get("object").and_then(Value::as_str).map(ToOwned::to_owned)
                                && let Some(linked_entity) = linked.get(&object_uri)
                            {
                                let mut linked_val = render_entity(linked_entity, language_filter)?;
                                apply_inline_join(&mut linked_val, linked, language_filter)?;
                                attr_obj.insert("entity".to_string(), linked_val);
                            }
                        } else if attr_type.as_deref() == Some("ListRelationship")
                            && let Some(object_list) = attr_obj.get("objectList").and_then(Value::as_array)
                        {
                            let entity_list = object_list
                                .iter()
                                .filter_map(Value::as_str)
                                .filter_map(|uri| linked.get(uri))
                                .map(|linked_entity| {
                                    let mut linked_val = render_entity(linked_entity, language_filter)?;
                                    apply_inline_join(&mut linked_val, linked, language_filter)?;
                                    Ok(linked_val)
                                })
                                .collect::<Result<Vec<_>, AndromedaError>>()?;
                            if !entity_list.is_empty() {
                                attr_obj.insert("entityList".to_string(), Value::Array(entity_list));
                            }
                        }
                    }
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    Ok(())
}

/// Convert a normalized rendered entity value into simplified/keyValues form.
pub fn simplify_value(val: &Value) -> Value {
    let Some(obj) = val.as_object() else { return val.clone() };

    let mut map = Map::new();
    for (key, value) in obj {
        match key.as_str() {
            "id" | "type" | "scope" | "@context" => {
                map.insert(key.clone(), value.clone());
            }
            _ => {
                map.insert(key.clone(), simplify_attr_value(value));
            }
        }
    }
    Value::Object(map)
}

fn simplify_attr_value(value: &Value) -> Value {
    match value {
        Value::Object(attr_obj) => match attr_obj.get("type").and_then(Value::as_str) {
            Some("Relationship") => {
                if let Some(entity) = attr_obj.get("entity") {
                    simplify_value(entity)
                } else {
                    attr_obj.get("object").cloned().unwrap_or_else(|| Value::Object(attr_obj.clone()))
                }
            }
            Some("Property") | Some("GeoProperty") | Some("VocabProperty") => attr_obj.get("value").cloned().unwrap_or_else(|| Value::Object(attr_obj.clone())),
            Some("LanguageProperty") => attr_obj.get("languageMap").cloned().unwrap_or_else(|| Value::Object(attr_obj.clone())),
            Some("JsonProperty") => attr_obj.get("json").cloned().unwrap_or_else(|| Value::Object(attr_obj.clone())),
            Some("ListProperty") => attr_obj.get("valueList").cloned().unwrap_or_else(|| Value::Object(attr_obj.clone())),
            Some("ListRelationship") => {
                if let Some(entity_list) = attr_obj.get("entityList") {
                    Value::Array(
                        entity_list
                            .as_array()
                            .map(|items| items.iter().map(simplify_value).collect())
                            .unwrap_or_default(),
                    )
                } else {
                    attr_obj.get("objectList").cloned().unwrap_or_else(|| Value::Object(attr_obj.clone()))
                }
            }
            Some(_) | None => Value::Object(attr_obj.clone()),
        },
        Value::Array(items) => Value::Array(items.iter().map(simplify_attr_value).collect()),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{render_entity, simplify_entity, simplify_value};
    use crate::{domain::entity::Entity, query::lang::LanguageFilter};
    use serde_json::{Value, json};

    fn parse_entity(value: Value) -> Entity {
        serde_json::from_value(value).expect("entity")
    }

    #[test]
    fn rendering_without_language_filter_matches_entity_serialization() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:000",
            "type": "Building",
            "name": {
                "type": "Property",
                "value": "Eiffel Tower"
            },
            "street": {
                "type": "LanguageProperty",
                "languageMap": {
                    "fr": "Grand Place",
                    "nl": "Grote Markt"
                }
            }
        }));

        let rendered = render_entity(&entity, None).expect("rendered entity");
        let serialized = serde_json::to_value(&entity).expect("serialized entity");

        assert_eq!(rendered, serialized);
    }

    #[test]
    fn simplifying_without_language_filter_matches_entity_simplification() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:004",
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {
                    "fr": "Grand Place",
                    "nl": "Grote Markt"
                }
            }
        }));

        let simplified = simplify_entity(&entity, None).expect("simplified entity");

        assert_eq!(simplified, entity.simplify());
    }

    #[test]
    fn language_filter_returns_exact_match_before_broader_match() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:001",
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {
                    "fr-CA": "Rue Principale",
                    "fr": "Grand Place"
                }
            }
        }));
        let filter = LanguageFilter::parse("fr").expect("language filter");

        let rendered = render_entity(&entity, Some(&filter)).expect("rendered entity");

        assert_eq!(rendered["street"]["type"], "Property");
        assert_eq!(rendered["street"]["value"], "Grand Place");
        assert_eq!(rendered["street"]["lang"], "fr");
    }

    #[test]
    fn language_filter_uses_wildcard_when_no_specific_match_exists() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:002",
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {
                    "fr": "Grand Place",
                    "nl": "Grote Markt"
                }
            }
        }));
        let filter = LanguageFilter::parse("*").expect("language filter");

        let rendered = render_entity(&entity, Some(&filter)).expect("rendered entity");

        assert_eq!(rendered["street"]["type"], "Property");
        assert_eq!(rendered["street"]["value"], "Grand Place");
        assert_eq!(rendered["street"]["lang"], "fr");
    }

    #[test]
    fn key_values_simplification_returns_scalar_after_language_negotiation() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:003",
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {
                    "fr": "Grand Place",
                    "nl": "Grote Markt"
                }
            }
        }));
        let filter = LanguageFilter::parse("fr").expect("language filter");

        let rendered = render_entity(&entity, Some(&filter)).expect("rendered entity");
        let simplified = simplify_value(&rendered);

        assert_eq!(simplified["street"], "Grand Place");
    }
}
