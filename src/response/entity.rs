//! Presentation / response-formatting logic for entity queries:
//! language negotiation, inline joins, keyValues simplification.

use crate::domain::{
    attribute::{Attribute, AttributeValue},
    entity::Entity,
};
use serde_json::Value;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Language negotiation
// ---------------------------------------------------------------------------

pub fn apply_language_negotiation(entity: &mut Entity, lang: &str) {
    let preferences = parse_lang_preferences(lang);
    if preferences.is_empty() {
        return;
    }

    let mut replacements = Vec::new();
    for (key, attr_val) in entity.attrs.iter() {
        match attr_val {
            AttributeValue::Single(Attribute::LanguageProperty(lp)) => {
                if let Some((matched_value, matched_tag)) = negotiate_language_map(&lp.language_map, &preferences) {
                    let mut prop = serde_json::Map::new();
                    prop.insert("type".to_string(), Value::String("Property".to_string()));
                    prop.insert("value".to_string(), Value::String(matched_value));
                    prop.insert("lang".to_string(), Value::String(matched_tag));
                    replacements.push((key.clone(), AttributeValue::Raw(Value::Object(prop))));
                }
            }
            AttributeValue::Multiple(list) => {
                let mut new_list_values = Vec::new();
                let mut any_changed = false;
                for attr in list {
                    if let Attribute::LanguageProperty(lp) = attr {
                        if let Some((matched_value, matched_tag)) = negotiate_language_map(&lp.language_map, &preferences) {
                            let mut prop = serde_json::Map::new();
                            prop.insert("type".to_string(), Value::String("Property".to_string()));
                            prop.insert("value".to_string(), Value::String(matched_value));
                            prop.insert("lang".to_string(), Value::String(matched_tag));
                            new_list_values.push(Value::Object(prop));
                            any_changed = true;
                        } else {
                            new_list_values.push(serde_json::to_value(attr).unwrap_or_default());
                        }
                    } else {
                        new_list_values.push(serde_json::to_value(attr).unwrap_or_default());
                    }
                }
                if any_changed {
                    replacements.push((key.clone(), AttributeValue::Raw(Value::Array(new_list_values))));
                }
            }
            _ => {}
        }
    }

    for (key, val) in replacements {
        entity.attrs.insert(key, val);
    }
}

struct LangPreference {
    tag: String,
    q: f64,
}

fn parse_lang_preferences(lang: &str) -> Vec<LangPreference> {
    let mut prefs = Vec::new();
    for part in lang.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let mut tag = part;
        let mut q = 1.0f64;
        if let Some(idx) = part.find(";q=") {
            tag = part[..idx].trim();
            if let Ok(qv) = part[idx + 3..].trim().parse::<f64>() {
                q = qv;
            }
        } else if let Some(idx) = part.find(";") {
            tag = part[..idx].trim();
        }
        prefs.push(LangPreference { tag: tag.to_string(), q });
    }
    prefs.sort_by(|a, b| b.q.partial_cmp(&a.q).unwrap_or(std::cmp::Ordering::Equal));
    prefs
}

fn negotiate_language_map(language_map: &Value, preferences: &[LangPreference]) -> Option<(String, String)> {
    let map = language_map.as_object()?;

    for pref in preferences {
        if pref.tag == "*" {
            if let Some((tag, val)) = map.iter().next() {
                return Some((val.as_str().unwrap_or("").to_string(), tag.clone()));
            }
        } else {
            if let Some(val) = map.get(&pref.tag) {
                return Some((val.as_str().unwrap_or("").to_string(), pref.tag.clone()));
            }
            let prefix = format!("{}-", pref.tag);
            for (tag, val) in map {
                if tag.starts_with(&prefix) || tag == &pref.tag {
                    return Some((val.as_str().unwrap_or("").to_string(), tag.clone()));
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Inline join (join=inline)
// ---------------------------------------------------------------------------

pub fn apply_inline_join(entity: &mut Value, linked: &HashMap<String, Entity>) {
    let Some(obj) = entity.as_object_mut() else { return };
    for (_key, attr_val) in obj.iter_mut() {
        match attr_val {
            Value::Object(attr_obj) => {
                let attr_type = attr_obj.get("type").and_then(|t| t.as_str()).map(String::from);
                if attr_type.as_deref() == Some("Relationship") {
                    if let Some(object_uri) = attr_obj.get("object").and_then(|o| o.as_str()).map(String::from)
                        && let Some(linked_entity) = linked.get(&object_uri)
                        && let Ok(mut linked_val) = serde_json::to_value(linked_entity)
                    {
                        apply_inline_join(&mut linked_val, linked);
                        attr_obj.insert("entity".to_string(), linked_val);
                    }
                } else if attr_type.as_deref() == Some("ListRelationship")
                    && let Some(object_list) = attr_obj.get("objectList").and_then(|o| o.as_array())
                {
                    let entity_list: Vec<Value> = object_list
                        .iter()
                        .filter_map(|uri_val| uri_val.as_str())
                        .filter_map(|uri| linked.get(uri))
                        .filter_map(|e| {
                            serde_json::to_value(e).ok().map(|mut v| {
                                apply_inline_join(&mut v, linked);
                                v
                            })
                        })
                        .collect();
                    if !entity_list.is_empty() {
                        attr_obj.insert("entityList".to_string(), Value::Array(entity_list));
                    }
                }
            }
            Value::Array(items) => {
                for item in items.iter_mut() {
                    if let Value::Object(attr_obj) = item {
                        let attr_type = attr_obj.get("type").and_then(|t| t.as_str()).map(String::from);
                        if attr_type.as_deref() == Some("Relationship") {
                            if let Some(object_uri) = attr_obj.get("object").and_then(|o| o.as_str()).map(String::from)
                                && let Some(linked_entity) = linked.get(&object_uri)
                                && let Ok(mut linked_val) = serde_json::to_value(linked_entity)
                            {
                                apply_inline_join(&mut linked_val, linked);
                                attr_obj.insert("entity".to_string(), linked_val);
                            }
                        } else if attr_type.as_deref() == Some("ListRelationship")
                            && let Some(object_list) = attr_obj.get("objectList").and_then(|o| o.as_array())
                        {
                            let entity_list: Vec<Value> = object_list
                                .iter()
                                .filter_map(|uri_val| uri_val.as_str())
                                .filter_map(|uri| linked.get(uri))
                                .filter_map(|e| {
                                    serde_json::to_value(e).ok().map(|mut v| {
                                        apply_inline_join(&mut v, linked);
                                        v
                                    })
                                })
                                .collect();
                            if !entity_list.is_empty() {
                                attr_obj.insert("entityList".to_string(), Value::Array(entity_list));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// keyValues / simplified format
// ---------------------------------------------------------------------------

pub fn simplify_value(val: &Value) -> Value {
    let Some(obj) = val.as_object() else { return val.clone() };

    let mut map = serde_json::Map::new();
    for (key, v) in obj {
        match key.as_str() {
            "id" | "type" | "scope" | "@context" => {
                map.insert(key.clone(), v.clone());
            }
            _ => {
                map.insert(key.clone(), simplify_attr_value(v));
            }
        }
    }
    Value::Object(map)
}

fn simplify_attr_value(v: &Value) -> Value {
    match v {
        Value::Object(attr_obj) => match attr_obj.get("type").and_then(|t| t.as_str()) {
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
                    Value::Array(entity_list.as_array().map(|arr| arr.iter().map(simplify_value).collect()).unwrap_or_default())
                } else {
                    attr_obj.get("objectList").cloned().unwrap_or_else(|| Value::Object(attr_obj.clone()))
                }
            }
            _ => Value::Object(attr_obj.clone()),
        },
        Value::Array(items) => Value::Array(items.iter().map(simplify_attr_value).collect()),
        _ => v.clone(),
    }
}
