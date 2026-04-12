//! Presentation / response-formatting logic for entity queries:
//! language negotiation, inline joins, concise/simplified rendering.
//!
//! The [`EntityRenderer`] encapsulates the full rendering pipeline so that
//! API handlers never call transformation functions directly. Configure via
//! builder methods and call [`EntityRenderer::render`] once.

use crate::{
    core::error::AndromedaError,
    domain::{
        attribute::{Attribute, AttributeValue, LanguageMapKey, LanguageMapValue, LanguageProperty},
        entity::Entity,
        serde_helpers,
    },
    http::model::language::{LanguageFilter, LanguageRange},
};
use chrono::{DateTime, SecondsFormat, Utc};
use indexmap::IndexMap;
use langtag::LangTagBuf;
use serde::Serialize;
use serde_json::{Map, Value};
use std::{collections::HashMap, ops::Deref};
use urn::Urn;

/// Output representation mode for entity rendering.
///
/// NGSI-LD defines three output representations beyond the internal expanded form
/// (ETSI GS CIM 009 clause 4.5.1, 4.5.4):
///
/// - **Normalized** -- full NGSI-LD representation with explicit type annotations.
/// - **Concise** -- lossless representation that omits redundant `"type"` members
///   when they can be inferred from the value key (clause 4.5.1 concise rules).
/// - **Simplified** -- lossy `keyValues` representation that collapses attributes
///   to their bare values (clause 4.5.4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RenderMode {
    /// Full NGSI-LD normalized representation with all type annotations.
    #[default]
    Normalized,
    /// Concise lossless representation: redundant type members omitted,
    /// but sub-attributes and metadata preserved (clause 4.5.1 concise rules).
    Concise,
    /// Simplified (keyValues) lossy representation: only Property values
    /// and Relationship objects, no metadata (clause 4.5.4).
    Simplified,
}

/// Render an entity to a response value, applying the optional NGSI-LD `lang`
/// filter, `join=inline`, and concise/simplified transformation at the
/// presentation boundary.
///
/// The rendering pipeline executes in order:
/// 1. Serialize entity (with language filter if set)
/// 2. Apply inline join (if linked entities provided)
/// 3. Apply concise/simplified transformation (if mode != Normalized)
pub struct EntityRenderer<'a> {
    entity: &'a Entity,
    language_filter: Option<&'a LanguageFilter>,
    inline_linked_entities: Option<&'a HashMap<String, Entity>>,
    mode: RenderMode,
}

impl<'a> EntityRenderer<'a> {
    /// Create a new renderer for the given entity in [`RenderMode::Normalized`] mode.
    pub const fn new(entity: &'a Entity) -> EntityRenderer<'a> {
        EntityRenderer {
            entity,
            language_filter: None,
            inline_linked_entities: None,
            mode: RenderMode::Normalized,
        }
    }

    /// Set the language filter for language property negotiation.
    pub const fn with_language_filter(mut self, filter: &'a LanguageFilter) -> Self {
        self.language_filter = Some(filter);
        self
    }

    /// Convenience: set the language filter from an `Option`, doing nothing if `None`.
    pub const fn with_optional_language_filter(mut self, filter: Option<&'a LanguageFilter>) -> Self {
        self.language_filter = filter;
        self
    }

    /// Set linked entities for `join=inline` expansion (NGSI-LD clause 4.11).
    pub const fn with_inline_linked_entities(mut self, linked: &'a HashMap<String, Entity>) -> Self {
        self.inline_linked_entities = Some(linked);
        self
    }

    /// Set the output representation mode.
    pub const fn with_mode(mut self, mode: RenderMode) -> Self {
        self.mode = mode;
        self
    }

    /// Execute the full rendering pipeline and return the JSON value.
    ///
    /// Pipeline order:
    /// 1. Serialize entity (with language negotiation if filter set)
    /// 2. Apply inline join (if linked entities provided)
    /// 3. Apply concise/simplified transformation (if mode != Normalized)
    pub fn render(self) -> Result<Value, AndromedaError> {
        // Fast path: simplified without language filter or inline join can use
        // the domain-level simplify() directly, avoiding a full serialize round-trip.
        if self.mode == RenderMode::Simplified && self.language_filter.is_none() && self.inline_linked_entities.is_none() {
            return Ok(self.entity.simplify());
        }

        // Step 1: serialize (with optional language negotiation)
        let mut value = if self.language_filter.is_none() {
            serialize_value(self.entity)?
        } else {
            let mut map = Map::new();
            map.insert("id".to_string(), serialize_value(&self.entity.id)?);
            map.insert("type".to_string(), serialize_value(&self.entity.type_)?);

            if let Some(scope) = &self.entity.scope {
                map.insert("scope".to_string(), serialize_value(scope)?);
            }
            if let Some(expires_at) = &self.entity.expires_at {
                map.insert("expiresAt".to_string(), Value::String(expires_at.to_rfc3339_opts(SecondsFormat::Secs, true)));
            }
            if let Some(location) = &self.entity.location {
                map.insert("location".to_string(), render_attribute_value(location, self.language_filter)?);
            }
            if let Some(observation_space) = &self.entity.observation_space {
                map.insert("observationSpace".to_string(), render_attribute_value(observation_space, self.language_filter)?);
            }
            if let Some(operation_space) = &self.entity.operation_space {
                map.insert("operationSpace".to_string(), render_attribute_value(operation_space, self.language_filter)?);
            }
            if let Some(created_at) = &self.entity.created_at {
                map.insert("createdAt".to_string(), Value::String(created_at.to_rfc3339_opts(SecondsFormat::Secs, true)));
            }
            if let Some(modified_at) = &self.entity.modified_at {
                map.insert("modifiedAt".to_string(), Value::String(modified_at.to_rfc3339_opts(SecondsFormat::Secs, true)));
            }
            if let Some(deleted_at) = &self.entity.deleted_at {
                map.insert("deletedAt".to_string(), Value::String(deleted_at.to_rfc3339_opts(SecondsFormat::Secs, true)));
            }

            for (key, attr) in &self.entity.attrs {
                map.insert(key.clone(), render_attribute_value(attr, self.language_filter)?);
            }

            Value::Object(map)
        };

        // Step 2: inline join
        if let Some(linked) = self.inline_linked_entities {
            apply_inline_join(&mut value, linked, self.language_filter)?;
        }

        // Step 3: concise/simplified transformation
        match self.mode {
            RenderMode::Normalized => {}
            RenderMode::Concise => {
                value = concise_value(&value);
            }
            RenderMode::Simplified => {
                value = simplify_value(&value);
            }
        }

        Ok(value)
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
        Attribute::Property(_)
        | Attribute::Relationship(_)
        | Attribute::VocabProperty(_)
        | Attribute::GeoProperty(_)
        | Attribute::JsonProperty(_)
        | Attribute::ListProperty(_)
        | Attribute::ListRelationship(_) => serialize_value(attribute),
    }
}

fn render_language_property(language_property: &LanguageProperty, language_filter: Option<&LanguageFilter>) -> Result<Value, AndromedaError> {
    if let Some(filter) = language_filter
        && let Some(negotiated) = negotiate_language_property(language_property, filter)
    {
        return serialize_value(&negotiated);
    }

    // Clone required: Attribute's #[serde(tag = "type")] adds the "type" discriminant during serialization.
    // Serializing LanguageProperty directly would omit the type tag.
    serialize_value(&Attribute::LanguageProperty(language_property.clone()))
}

fn negotiate_language_property<'a>(language_property: &'a LanguageProperty, filter: &LanguageFilter) -> Option<NegotiatedProperty<'a>> {
    let available = language_entries(&language_property.language_map);

    for preference in filter.ordered() {
        let range = preference.value();
        match range {
            LanguageRange::Wildcard => {
                if let Some(entry) = available.iter().find(|entry| matches!(entry.range, AvailableLanguageRange::Tag(_))) {
                    return Some(NegotiatedProperty::from_language_property(language_property, entry));
                }
                if let Some(entry) = available.iter().find(|entry| matches!(entry.range, AvailableLanguageRange::Default)) {
                    return Some(NegotiatedProperty::from_language_property(language_property, entry));
                }
            }
            LanguageRange::Tag(_) => {
                // Exact match
                if let Some(entry) = available.iter().find(|entry| entry.matches_exact(range)) {
                    return Some(NegotiatedProperty::from_language_property(language_property, entry));
                }
                // Prefix match (only for basic ranges like "fr", not "fr-CA")
                if range.is_basic_range()
                    && let Some(entry) = available.iter().find(|entry| entry.matches_prefix(range))
                {
                    return Some(NegotiatedProperty::from_language_property(language_property, entry));
                }
            }
        }
    }

    None
}

fn language_entries(language_map: &IndexMap<LanguageMapKey, LanguageMapValue>) -> Vec<AvailableLanguage<'_>> {
    language_map.iter().map(|(key, value)| AvailableLanguage::from_entry(key, value)).collect()
}

fn serialize_value<T>(value: &T) -> Result<Value, AndromedaError>
where
    T: Serialize,
{
    serde_json::to_value(value).map_err(|error| AndromedaError::internal(format!("Failed to serialize response value: {}", error)))
}

#[derive(Debug)]
struct AvailableLanguage<'a> {
    range: AvailableLanguageRange,
    value: &'a LanguageMapValue,
}

impl<'a> AvailableLanguage<'a> {
    /// Construct from a typed language map entry. Infallible because keys are
    /// already validated at deserialization time.
    fn from_entry(key: &LanguageMapKey, value: &'a LanguageMapValue) -> AvailableLanguage<'a> {
        let range = match key {
            LanguageMapKey::None => AvailableLanguageRange::Default,
            LanguageMapKey::Tag(tag) => AvailableLanguageRange::Tag(tag.clone()),
        };
        AvailableLanguage { range, value }
    }

    /// Case-insensitive exact match using [`LanguageRange::exact_matches`].
    fn matches_exact(&self, range: &LanguageRange) -> bool {
        match &self.range {
            AvailableLanguageRange::Tag(tag) => range.exact_matches(tag),
            AvailableLanguageRange::Default => false,
        }
    }

    /// RFC 4647 basic filtering prefix match using [`LanguageRange::prefix_matches`].
    fn matches_prefix(&self, range: &LanguageRange) -> bool {
        match &self.range {
            AvailableLanguageRange::Tag(tag) => range.prefix_matches(tag),
            AvailableLanguageRange::Default => false,
        }
    }
}

#[derive(Debug, Clone)]
enum AvailableLanguageRange {
    Default,
    Tag(LangTagBuf),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NegotiatedProperty<'a> {
    #[serde(rename = "type")]
    type_: &'static str,
    value: &'a LanguageMapValue,
    lang: String,
    #[serde(skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    observed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dataset_id: &'a Option<Urn>,
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
            observed_at: language_property.common.observed_at,
            dataset_id: &language_property.common.dataset_id,
            sub_attributes: &language_property.sub_attributes,
        }
    }
}

/// Convert a normalized rendered entity value into concise form (NGSI-LD clause 4.5.1).
///
/// Concise representation removes redundant `"type"` keys from attributes when the
/// attribute kind can be inferred from its value key. Sub-attributes and metadata
/// are preserved. Entity-level keys (`id`, `type`, `scope`, `@context`) pass through
/// unchanged.
pub fn concise_value(val: &Value) -> Value {
    let Some(obj) = val.as_object() else { return val.clone() };

    let mut map = Map::new();
    for (key, value) in obj {
        match key.as_str() {
            "id" | "type" | "scope" | "@context" | "createdAt" | "modifiedAt" | "deletedAt" | "expiresAt" => {
                map.insert(key.clone(), value.clone());
            }
            _ => {
                map.insert(key.clone(), concise_attr_value(value));
            }
        }
    }
    Value::Object(map)
}

/// Apply concise transformation to an attribute value (single instance or multi-attribute array).
fn concise_attr_value(value: &Value) -> Value {
    match value {
        Value::Object(attr_obj) => concise_single_instance(attr_obj),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| match item {
                    Value::Object(attr_obj) => concise_single_instance(attr_obj),
                    other => other.clone(),
                })
                .collect(),
        ),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => value.clone(),
    }
}

/// Concise a single attribute instance object.
///
/// Rules per NGSI-LD clause 4.5.1:
/// - If no sub-attributes exist: collapse to just the value
///   (Property -> value, Relationship -> `{"object": ...}`, etc.)
/// - If sub-attributes exist: keep as object but remove the `"type"` key,
///   recursively concise sub-attributes
fn concise_single_instance(attr_obj: &Map<String, Value>) -> Value {
    let attr_type = attr_obj.get("type").and_then(Value::as_str);

    let value_key = match attr_type {
        Some("Property") | Some("GeoProperty") => "value",
        Some("Relationship") => "object",
        Some("LanguageProperty") => "languageMap",
        Some("VocabProperty") => "vocab",
        Some("JsonProperty") => "json",
        Some("ListProperty") => "valueList",
        Some("ListRelationship") => "objectList",
        // Unknown type -- pass through unchanged
        Some(_) | None => return Value::Object(attr_obj.clone()),
    };

    // Check if the attribute has only `type` + value key (bare minimum for collapse)
    let is_bare = attr_obj.len() == 2 && attr_obj.contains_key("type") && attr_obj.contains_key(value_key);

    if is_bare {
        attr_obj.get(value_key).cloned().unwrap_or_else(|| Value::Object(attr_obj.clone()))
    } else {
        // Has metadata and/or sub-attributes: remove "type", recursively concise sub-attrs
        let metadata_keys: &[&str] = &[
            value_key,
            "observedAt",
            "datasetId",
            "unitCode",
            "createdAt",
            "modifiedAt",
            "expiresAt",
            "instanceId",
        ];
        let mut result = Map::new();
        for (k, v) in attr_obj {
            if k == "type" {
                continue;
            }
            if metadata_keys.contains(&k.as_str()) {
                result.insert(k.clone(), v.clone());
            } else {
                // Sub-attribute: recursively concise
                result.insert(k.clone(), concise_attr_value(v));
            }
        }
        Value::Object(result)
    }
}

/// Apply `join=inline` expansion to an already-rendered entity response.
///
/// This is kept `pub` for the notification worker which operates on pre-compacted
/// values rather than `Entity` structs.
pub fn apply_inline_join(entity: &mut Value, linked: &HashMap<String, Entity>, language_filter: Option<&LanguageFilter>) -> Result<(), AndromedaError> {
    let Some(obj) = entity.as_object_mut() else { return Ok(()) };
    for attr_val in obj.values_mut() {
        match attr_val {
            Value::Object(attr_obj) => {
                inline_join_attribute_instance(attr_obj, linked, language_filter)?;
            }
            Value::Array(items) => {
                for item in items.iter_mut() {
                    if let Value::Object(attr_obj) = item {
                        inline_join_attribute_instance(attr_obj, linked, language_filter)?;
                    }
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    Ok(())
}

/// Expand a single attribute instance with inline-joined linked entities.
///
/// For `Relationship` attributes, resolves the `object` URI against the linked
/// entity map and inserts the rendered entity under the `"entity"` key.
/// For `ListRelationship` attributes, resolves each URI in `objectList` and
/// inserts the rendered entities under the `"entityList"` key.
fn inline_join_attribute_instance(
    attr_obj: &mut Map<String, Value>,
    linked: &HashMap<String, Entity>,
    language_filter: Option<&LanguageFilter>,
) -> Result<(), AndromedaError> {
    let attr_type = attr_obj.get("type").and_then(Value::as_str).map(ToOwned::to_owned);
    if attr_type.as_deref() == Some("Relationship") {
        if let Some(object_uri) = attr_obj.get("object").and_then(Value::as_str).map(ToOwned::to_owned)
            && let Some(linked_entity) = linked.get(&object_uri)
        {
            let mut rendered = EntityRenderer::new(linked_entity);
            if let Some(filter) = language_filter {
                rendered = rendered.with_language_filter(filter);
            }
            let mut linked_val = rendered.render()?;
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
                let mut rendered = EntityRenderer::new(linked_entity);
                if let Some(filter) = language_filter {
                    rendered = rendered.with_language_filter(filter);
                }
                let mut linked_val = rendered.render()?;
                apply_inline_join(&mut linked_val, linked, language_filter)?;
                Ok(linked_val)
            })
            .collect::<Result<Vec<_>, AndromedaError>>()?;
        if !entity_list.is_empty() {
            attr_obj.insert("entityList".to_string(), Value::Array(entity_list));
        }
    }
    Ok(())
}

/// Convert a normalized rendered entity value into simplified/keyValues form
/// (NGSI-LD clause 4.5.4).
///
/// This is kept `pub` for the notification worker which operates on pre-compacted
/// values rather than `Entity` structs.
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

/// Decode percent-encoded URIs in Relationship `object` fields.
///
/// Postgres stores URIs as-is, but some clients percent-encode characters in
/// entity IDs (e.g. colons). This function decodes them for human-readable
/// response output. Applied to JSON/LD+JSON responses but not GeoJSON (which
/// wraps the entity in a Feature envelope).
pub fn decode_percent_encoded_uris(value: &mut Value) {
    match value {
        Value::Object(obj) => {
            if obj.get("type").and_then(|t| t.as_str()) == Some("Relationship")
                && let Some(Value::String(uri)) = obj.get_mut("object")
                && uri.contains('%')
                && let Ok(decoded) = percent_encoding::percent_decode_str(uri).decode_utf8()
            {
                *uri = decoded.into_owned();
            }
            for v in obj.values_mut() {
                decode_percent_encoded_uris(v);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                decode_percent_encoded_uris(v);
            }
        }
        // Scalars have no URIs to decode
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Render a snapshot or pre-serialized entity value by applying attribute filtering
/// and representation mode transformation.
///
/// Used for snapshot-scoped responses (ETSI GS CIM 009 clause 5.5.15) where
/// the entity data comes from frozen JSONB storage rather than the domain model.
/// The [`EntityRenderer`] pipeline is not applicable because there is no `Entity`
/// struct -- only a raw `Value`.
///
/// - `attrs`: if `Some`, only retain these attribute keys (plus `id`, `type`, `@context`).
/// - `mode`: apply simplified/concise/normalized transformation.
pub fn render_snapshot(mut value: Value, attrs: Option<&[&str]>, mode: RenderMode) -> Value {
    if let Some(requested) = attrs
        && let Some(obj) = value.as_object_mut()
    {
        let keys_to_keep: std::collections::HashSet<&str> = ["id", "type", "@context"].iter().copied().chain(requested.iter().copied()).collect();
        obj.retain(|k, _| keys_to_keep.iter().any(|keep| k.ends_with(keep)));
    }

    match mode {
        RenderMode::Simplified => simplify_value(&value),
        RenderMode::Concise => concise_value(&value),
        RenderMode::Normalized => value,
    }
}

#[cfg(test)]
mod tests {
    use super::{EntityRenderer, RenderMode, concise_value, decode_percent_encoded_uris, render_snapshot};
    use crate::{domain::entity::Entity, http::model::language::LanguageFilter};
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

        let rendered = EntityRenderer::new(&entity).render().expect("rendered entity");
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

        let simplified = EntityRenderer::new(&entity)
            .with_mode(RenderMode::Simplified)
            .render()
            .expect("simplified entity");

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

        let rendered = EntityRenderer::new(&entity).with_language_filter(&filter).render().expect("rendered entity");

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

        let rendered = EntityRenderer::new(&entity).with_language_filter(&filter).render().expect("rendered entity");

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

        let simplified = EntityRenderer::new(&entity)
            .with_language_filter(&filter)
            .with_mode(RenderMode::Simplified)
            .render()
            .expect("rendered entity");

        assert_eq!(simplified["street"], "Grand Place");
    }

    // --- Concise rendering tests (NGSI-LD clause 4.5.1) ---

    #[test]
    fn concise_property_without_sub_attrs_collapses_to_value() {
        let input = json!({"id": "urn:x", "type": "T", "temp": {"type": "Property", "value": 25}});
        let result = concise_value(&input);
        assert_eq!(result["temp"], json!(25));
        assert_eq!(result["id"], "urn:x");
        assert_eq!(result["type"], "T");
    }

    #[test]
    fn concise_property_with_sub_attrs_removes_type() {
        let input = json!({
            "id": "urn:x", "type": "T",
            "temp": {
                "type": "Property",
                "value": 121,
                "observedAt": "2024-01-01T00:00:00Z",
                "reliability": {"type": "Property", "value": 0.7}
            }
        });
        let result = concise_value(&input);
        let temp = result.get("temp").expect("temp");
        assert!(temp.get("type").is_none(), "type key should be removed");
        assert_eq!(temp["value"], 121);
        assert_eq!(temp["observedAt"], "2024-01-01T00:00:00Z");
        // Sub-attribute should be recursively concised
        assert_eq!(temp["reliability"], json!(0.7));
    }

    #[test]
    fn concise_relationship_without_sub_attrs() {
        let input = json!({"id": "urn:x", "type": "T", "ref": {"type": "Relationship", "object": "urn:y"}});
        let result = concise_value(&input);
        assert_eq!(result["ref"], json!("urn:y"));
    }

    #[test]
    fn concise_relationship_with_sub_attrs() {
        let input = json!({
            "id": "urn:x", "type": "T",
            "ref": {"type": "Relationship", "object": "urn:y", "confidence": {"type": "Property", "value": 0.9}}
        });
        let result = concise_value(&input);
        let r = result.get("ref").expect("ref");
        assert!(r.get("type").is_none());
        assert_eq!(r["object"], "urn:y");
        assert_eq!(r["confidence"], json!(0.9));
    }

    #[test]
    fn concise_geo_property_without_sub_attrs() {
        let input = json!({
            "id": "urn:x", "type": "T",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2]}}
        });
        let result = concise_value(&input);
        assert_eq!(result["location"], json!({"type": "Point", "coordinates": [1, 2]}));
    }

    #[test]
    fn concise_language_property() {
        let input = json!({
            "id": "urn:x", "type": "T",
            "name": {"type": "LanguageProperty", "languageMap": {"en": "Tower"}}
        });
        let result = concise_value(&input);
        assert_eq!(result["name"], json!({"en": "Tower"}));
    }

    #[test]
    fn concise_json_property() {
        let input = json!({
            "id": "urn:x", "type": "T",
            "config": {"type": "JsonProperty", "json": {"key": "val"}}
        });
        let result = concise_value(&input);
        assert_eq!(result["config"], json!({"key": "val"}));
    }

    #[test]
    fn concise_vocab_property() {
        let input = json!({
            "id": "urn:x", "type": "T",
            "category": {"type": "VocabProperty", "vocab": "commercial"}
        });
        let result = concise_value(&input);
        assert_eq!(result["category"], json!("commercial"));
    }

    #[test]
    fn concise_list_property() {
        let input = json!({
            "id": "urn:x", "type": "T",
            "tags": {"type": "ListProperty", "valueList": ["a", "b"]}
        });
        let result = concise_value(&input);
        assert_eq!(result["tags"], json!(["a", "b"]));
    }

    #[test]
    fn concise_list_relationship() {
        let input = json!({
            "id": "urn:x", "type": "T",
            "refs": {"type": "ListRelationship", "objectList": ["urn:a", "urn:b"]}
        });
        let result = concise_value(&input);
        assert_eq!(result["refs"], json!(["urn:a", "urn:b"]));
    }

    #[test]
    fn concise_preserves_id_type_scope_context() {
        let input = json!({
            "id": "urn:x",
            "type": "T",
            "scope": ["/A"],
            "@context": "https://example.org/ctx",
            "temp": {"type": "Property", "value": 10}
        });
        let result = concise_value(&input);
        assert_eq!(result["id"], "urn:x");
        assert_eq!(result["type"], "T");
        assert_eq!(result["scope"], json!(["/A"]));
        assert_eq!(result["@context"], "https://example.org/ctx");
    }

    #[test]
    fn concise_multi_attribute_array() {
        let input = json!({
            "id": "urn:x", "type": "T",
            "temp": [
                {"type": "Property", "value": 25, "datasetId": "urn:ds:1"},
                {"type": "Property", "value": 30, "datasetId": "urn:ds:2"}
            ]
        });
        let result = concise_value(&input);
        let arr = result["temp"].as_array().expect("array");
        assert_eq!(arr.len(), 2);
        // Each instance should be concised individually -- with datasetId they have metadata
        // so they keep the value key but lose "type"
        assert!(arr[0].get("type").is_none());
        assert_eq!(arr[0]["value"], 25);
        assert_eq!(arr[0]["datasetId"], "urn:ds:1");
    }

    #[test]
    fn concise_via_renderer_mode() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:010",
            "type": "Building",
            "name": {"type": "Property", "value": "Tower"}
        }));
        let result = EntityRenderer::new(&entity).with_mode(RenderMode::Concise).render().expect("concise");
        assert_eq!(result["name"], "Tower");
    }

    #[test]
    fn render_mode_with_mode_method() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:011",
            "type": "Building",
            "name": {"type": "Property", "value": "Tower"}
        }));
        let result = EntityRenderer::new(&entity).with_mode(RenderMode::Simplified).render().expect("simplified");
        assert_eq!(result["name"], "Tower");
    }

    #[test]
    fn with_optional_language_filter_none_has_no_effect() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:012",
            "type": "Building",
            "name": {"type": "Property", "value": "Tower"}
        }));
        let result = EntityRenderer::new(&entity).with_optional_language_filter(None).render().expect("rendered");
        let expected = serde_json::to_value(&entity).expect("serialized");
        assert_eq!(result, expected);
    }

    #[test]
    fn with_optional_language_filter_some_applies_filter() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:013",
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}
            }
        }));
        let filter = LanguageFilter::parse("fr").expect("filter");
        let result = EntityRenderer::new(&entity)
            .with_optional_language_filter(Some(&filter))
            .render()
            .expect("rendered");
        assert_eq!(result["street"]["lang"], "fr");
    }

    // --- decode_percent_encoded_uris tests ---

    #[test]
    fn decode_percent_encoded_relationship_object() {
        let mut value = json!({
            "id": "urn:x", "type": "T",
            "ref": {"type": "Relationship", "object": "urn%3Angsi-ld%3ABuilding%3A1"}
        });
        decode_percent_encoded_uris(&mut value);
        assert_eq!(value["ref"]["object"], "urn:ngsi-ld:Building:1");
    }

    #[test]
    fn decode_percent_encoded_uris_skips_non_relationships() {
        let mut value = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "test%20value"}});
        let original = value.clone();
        decode_percent_encoded_uris(&mut value);
        assert_eq!(value, original);
    }

    #[test]
    fn decode_percent_encoded_uris_recursive_in_arrays() {
        let mut value = json!([
            {"type": "Relationship", "object": "urn%3Ax"},
            {"type": "Relationship", "object": "urn:y"}
        ]);
        decode_percent_encoded_uris(&mut value);
        assert_eq!(value[0]["object"], "urn:x");
        assert_eq!(value[1]["object"], "urn:y");
    }

    // --- render_snapshot tests ---

    #[test]
    fn render_snapshot_filters_attrs() {
        let value = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "x"}, "temp": {"type": "Property", "value": 25}});
        let result = render_snapshot(value, Some(&["name"]), RenderMode::Normalized);
        assert!(result.get("name").is_some());
        assert!(result.get("temp").is_none());
        assert!(result.get("id").is_some());
        assert!(result.get("type").is_some());
    }

    #[test]
    fn render_snapshot_applies_simplified_mode() {
        let value = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "Tower"}});
        let result = render_snapshot(value, None, RenderMode::Simplified);
        assert_eq!(result["name"], "Tower");
    }

    #[test]
    fn render_snapshot_applies_concise_mode() {
        let value = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "Tower"}});
        let result = render_snapshot(value, None, RenderMode::Concise);
        assert_eq!(result["name"], "Tower");
    }

    #[test]
    fn render_snapshot_normalized_mode_is_identity() {
        let value = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "Tower"}});
        let result = render_snapshot(value.clone(), None, RenderMode::Normalized);
        assert_eq!(result, value);
    }

    #[test]
    fn render_snapshot_filters_then_transforms() {
        let value = json!({"id": "urn:x", "type": "T", "name": {"type": "Property", "value": "Tower"}, "temp": {"type": "Property", "value": 25}});
        let result = render_snapshot(value, Some(&["name"]), RenderMode::Simplified);
        assert_eq!(result["name"], "Tower");
        assert!(result.get("temp").is_none());
    }

    // --- apply_inline_join tests (NGSI-LD clause 4.11) ---

    use super::apply_inline_join;
    use std::collections::HashMap;

    #[test]
    fn inline_join_expands_relationship() {
        let linked_entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Room:1",
            "type": "Room",
            "name": {"type": "Property", "value": "Conference Room"}
        }));
        let mut linked_map = HashMap::new();
        linked_map.insert("urn:ngsi-ld:Room:1".to_string(), linked_entity);

        let mut entity = json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "hasRoom": {"type": "Relationship", "object": "urn:ngsi-ld:Room:1"}
        });

        apply_inline_join(&mut entity, &linked_map, None).expect("inline join");
        assert!(entity["hasRoom"]["entity"].is_object());
        assert_eq!(entity["hasRoom"]["entity"]["id"], "urn:ngsi-ld:Room:1");
        assert_eq!(entity["hasRoom"]["entity"]["name"]["value"], "Conference Room");
    }

    #[test]
    fn inline_join_expands_list_relationship() {
        let room_a = parse_entity(json!({"id": "urn:a", "type": "Room", "name": {"type": "Property", "value": "A"}}));
        let room_b = parse_entity(json!({"id": "urn:b", "type": "Room", "name": {"type": "Property", "value": "B"}}));
        let mut linked_map = HashMap::new();
        linked_map.insert("urn:a".to_string(), room_a);
        linked_map.insert("urn:b".to_string(), room_b);

        let mut entity = json!({
            "id": "urn:building", "type": "Building",
            "hasRooms": {"type": "ListRelationship", "objectList": ["urn:a", "urn:b"]}
        });

        apply_inline_join(&mut entity, &linked_map, None).expect("inline join");
        let entity_list = entity["hasRooms"]["entityList"].as_array().expect("entityList array");
        assert_eq!(entity_list.len(), 2);
    }

    #[test]
    fn inline_join_skips_unresolved() {
        let linked_map: HashMap<String, Entity> = HashMap::new();
        let mut entity = json!({
            "id": "urn:building", "type": "Building",
            "hasRoom": {"type": "Relationship", "object": "urn:ngsi-ld:Missing:1"}
        });

        apply_inline_join(&mut entity, &linked_map, None).expect("inline join");
        assert!(entity["hasRoom"].get("entity").is_none());
    }

    #[test]
    fn inline_join_handles_multi_attribute_array() {
        let room = parse_entity(json!({"id": "urn:a", "type": "Room", "name": {"type": "Property", "value": "A"}}));
        let mut linked_map = HashMap::new();
        linked_map.insert("urn:a".to_string(), room);

        let mut entity = json!({
            "id": "urn:building", "type": "Building",
            "hasRoom": [
                {"type": "Relationship", "object": "urn:a", "datasetId": "urn:ds:1"},
                {"type": "Relationship", "object": "urn:missing", "datasetId": "urn:ds:2"}
            ]
        });

        apply_inline_join(&mut entity, &linked_map, None).expect("inline join");
        let arr = entity["hasRoom"].as_array().expect("array");
        assert!(arr[0].get("entity").is_some());
        assert!(arr[1].get("entity").is_none());
    }

    #[test]
    fn inline_join_recursive() {
        let entity_c = parse_entity(json!({
            "id": "urn:c", "type": "T", "name": {"type": "Property", "value": "C"}
        }));
        let entity_b = parse_entity(json!({
            "id": "urn:b", "type": "T",
            "ref": {"type": "Relationship", "object": "urn:c"}
        }));
        let mut linked_map = HashMap::new();
        linked_map.insert("urn:b".to_string(), entity_b);
        linked_map.insert("urn:c".to_string(), entity_c);

        let mut entity = json!({
            "id": "urn:a", "type": "T",
            "ref": {"type": "Relationship", "object": "urn:b"}
        });

        apply_inline_join(&mut entity, &linked_map, None).expect("inline join");
        assert_eq!(entity["ref"]["entity"]["id"], "urn:b");
        assert_eq!(entity["ref"]["entity"]["ref"]["entity"]["id"], "urn:c");
    }
}
