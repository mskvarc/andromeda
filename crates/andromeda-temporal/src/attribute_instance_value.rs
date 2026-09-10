//! The temporal value one expanded NGSI-LD attribute instance carries, and the
//! metadata that travels beside it.
//!
//! One decoder per attribute family (clause 4.5.1 and the per-family clauses
//! 4.5.2, 4.5.3, 4.5.18, 4.5.20, 4.5.21, 4.5.22 and 4.5.24), because the member
//! a family's value lives under and the JSON-LD keyword that member's nodes
//! carry are both family-decided. Everything the family does not name as its
//! value member is metadata and is kept verbatim.

use crate::error::TemporalError;
use andromeda_ngsi_ld_lexicon::{
    attribute_type::AttributeType,
    ngsi_ld_date::NgsiLdDate,
    ngsi_ld_date_time::NgsiLdDateTime,
    ngsi_ld_time::NgsiLdTime,
    uri::{Uri, UriParseError},
};
use andromeda_ngsi_ld_model::{expanded::geo_property::expanded_geo_node_to_geometry, temporal::TemporalValue};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_render::uri_decoding::decode_percent_encoded_uri;
use andromeda_ngsi_ld_vocabulary::expanded::{classes as expanded_classes, properties as expanded_properties};
use serde_json::{Map, Value};

/// Retain expanded members that are metadata rather than dedicated value fields.
pub(crate) fn collect_metadata(instance: &Value) -> Option<Value> {
    let object = instance.as_object()?;
    let metadata = object
        .iter()
        .filter(|(key, _)| !is_dedicated_key(key))
        .map(|(key, value)| (key.clone(), value.clone())) // clone: metadata owns the retained expanded members.
        .collect::<Map<String, Value>>();
    (!metadata.is_empty()).then_some(Value::Object(metadata))
}

/// Return whether a member is persisted in a dedicated temporal column.
fn is_dedicated_key(key: &str) -> bool {
    key == "@id"
        || key == "@type"
        || key == expanded_properties::DATASET_ID.as_str()
        || key == expanded_properties::JSON.as_str()
        || key == expanded_properties::LANGUAGE_MAP.as_str()
        || key == expanded_properties::OBJECT.as_str()
        || key == expanded_properties::OBJECTS.as_str()
        // Clause 4.5.9 holds the list families under their own members; without
        // these a list instance's value is copied into the metadata blob as well
        // and echoed back over the rendered value.
        || key == expanded_properties::OBJECT_LIST.as_str()
        || key == expanded_properties::OBJECT_LISTS.as_str()
        || key == expanded_properties::VALUE.as_str()
        || key == expanded_properties::VALUES.as_str()
        || key == expanded_properties::VALUE_LIST.as_str()
        || key == expanded_properties::VALUE_LISTS.as_str()
        || key == expanded_properties::VOCAB.as_str()
        || key == expanded_properties::VOCABS.as_str()
        || key == expanded_properties::INSTANCE_ID.as_str()
        || key == expanded_properties::OBSERVED_AT.as_str()
        || key == expanded_properties::LANGUAGE_MAPS.as_str()
        || key == expanded_properties::JSONS.as_str()
}

/// Decode one expanded attribute value into its neutral temporal form.
///
/// # Errors
///
/// [`TemporalError::Validation`] when the instance carries no member of the
/// shape its declared family requires, or when a value that has to be a URI, a
/// number or one of the three clause 4.6.3 temporal forms is not one.
pub(crate) fn extract_temporal_value(instance: &Value, attribute_type: AttributeType) -> Result<TemporalValue, TemporalError> {
    match attribute_type {
        AttributeType::Relationship => relationship_value(instance),
        AttributeType::GeoProperty => geometry_value(instance),
        AttributeType::VocabProperty => vocab_value(instance),
        AttributeType::LanguageProperty => owned_member(instance, expanded_properties::LANGUAGE_MAP.as_str(), "LanguageProperty missing languageMap"),
        AttributeType::JsonProperty => nested_value(instance, expanded_properties::JSON.as_str(), "JsonProperty missing json value"),
        AttributeType::ListProperty => list_value(instance, expanded_properties::VALUE_LIST.as_str(), "ListProperty missing valueList"),
        AttributeType::ListRelationship => list_relationship_value(instance),
        AttributeType::Property => property_value(instance),
    }
}

/// Decode every target a Relationship instance points at.
///
/// Clause 4.5.3.2 defines `object` as "the Relationship's object represented by
/// a URI **or array of URIs**", and clause 4.5.9 EXAMPLE 7 shows a temporal
/// evolution whose `objects` pairs each hold an array of URIs. Every `@id` node
/// of the member is therefore decoded: a single target keeps the URI shape the
/// "URI" column of Table 4.5.19.1-2 describes, and several are held as the
/// ordered array of URIs the clause admits. That array is aggregated by the
/// attribute family through Table 4.5.19.1-3 rather than by its size, so
/// holding it as a JSON array does not hand a Relationship the array-size
/// aggregates of Table 4.5.19.1-1.
fn relationship_value(instance: &Value) -> Result<TemporalValue, TemporalError> {
    let nodes = instance
        .get(expanded_properties::OBJECT.as_str())
        .and_then(Value::as_array)
        .ok_or_else(|| TemporalError::validation("Relationship missing object"))?;
    let mut targets = nodes
        .iter()
        .map(|node| {
            let raw = node
                .get("@id")
                .and_then(Value::as_str)
                .ok_or_else(|| TemporalError::validation("Relationship object names no target URI"))?;
            parse_target(raw, "Relationship object")
        })
        .collect::<Result<Vec<Uri>, TemporalError>>()?;
    match targets.pop() {
        Some(single) if targets.is_empty() => Ok(TemporalValue::Uri(single)),
        Some(last) => {
            targets.push(last);
            Ok(TemporalValue::Object(uri_array(&targets)))
        }
        None => Err(TemporalError::validation("Relationship missing object")),
    }
}

/// Decode an expanded or encoded `GeoProperty` value.
fn geometry_value(instance: &Value) -> Result<TemporalValue, TemporalError> {
    let node = first_member(instance, expanded_properties::VALUE.as_str()).ok_or_else(|| TemporalError::validation("GeoProperty missing value"))?;
    let geometry = if let Some(value) = node.get("@value") {
        match value.as_str() {
            Some(raw) => serde_json::from_str(raw).map_err(|error| TemporalError::validation(format!("invalid GeoProperty GeoJSON string: {error}")))?,
            None => Value::clone(value), // clone: the intent owns the geometry value.
        }
    } else {
        let geometry = expanded_geo_node_to_geometry(node).ok_or_else(|| TemporalError::validation("invalid expanded GeoProperty geometry"))?;
        serde_json::to_value(geometry.to_geojson()).map_err(|error| TemporalError::internal(format!("failed to serialize GeoProperty geometry: {error}")))?
    };
    Ok(TemporalValue::Geometry(geometry))
}

/// Decode one or many `VocabProperty` identifiers.
fn vocab_value(instance: &Value) -> Result<TemporalValue, TemporalError> {
    let values = instance
        .get(expanded_properties::VOCAB.as_str())
        .and_then(Value::as_array)
        .ok_or_else(|| TemporalError::validation("VocabProperty missing vocab value"))?;
    if values.len() > 1 {
        Ok(TemporalValue::Object(Value::Array(
            values
                .iter()
                .filter_map(|value| value.get("@id").or_else(|| value.get("@value")).cloned())
                .collect(),
        )))
    } else {
        values
            .first()
            .and_then(|value| value.get("@id").or_else(|| value.get("@value")))
            .and_then(Value::as_str)
            .map(|value| TemporalValue::String(value.to_owned()))
            .ok_or_else(|| TemporalError::validation("VocabProperty missing vocab value"))
    }
}

/// Own an expanded member used directly as a temporal object value.
fn owned_member(instance: &Value, key: &str, error: &'static str) -> Result<TemporalValue, TemporalError> {
    instance
        .get(key)
        .cloned()
        .map(TemporalValue::Object)
        .ok_or_else(|| TemporalError::validation(error))
}

/// Own the `@value` nested in the first expanded member.
fn nested_value(instance: &Value, key: &str, error: &'static str) -> Result<TemporalValue, TemporalError> {
    first_member(instance, key)
        .and_then(|value| value.get("@value"))
        .cloned()
        .map(TemporalValue::Object)
        .ok_or_else(|| TemporalError::validation(error))
}

/// Own the `@list` nested in the first expanded member.
fn list_value(instance: &Value, key: &str, error: &'static str) -> Result<TemporalValue, TemporalError> {
    first_member(instance, key)
        .and_then(|value| value.get("@list"))
        .cloned()
        .map(TemporalValue::Object)
        .ok_or_else(|| TemporalError::validation(error))
}

/// Decode a `ListRelationship` object list.
fn list_relationship_value(instance: &Value) -> Result<TemporalValue, TemporalError> {
    let list = first_member(instance, expanded_properties::OBJECT_LIST.as_str())
        .and_then(|value| value.get("@list"))
        .and_then(Value::as_array)
        .ok_or_else(|| TemporalError::validation("ListRelationship missing objectList"))?;
    let targets = list
        .iter()
        .map(|member| {
            let raw = object_list_target_iri(member).ok_or_else(|| TemporalError::validation("ListRelationship objectList member names no target URI"))?;
            parse_target(raw, "ListRelationship objectList member")
        })
        .collect::<Result<Vec<Uri>, TemporalError>>()?;
    Ok(TemporalValue::Object(uri_array(&targets)))
}

/// The target IRI one expanded `objectList` member names, in any of the three
/// spellings a conformant document produces.
///
/// Clause 4.5.22.2 makes the normalized member "a JSON object containing a
/// single Attribute with a key called `object`", which expands to a node
/// carrying `hasObject`. Clause 4.5.22.3 admits the bare URI string beside it,
/// and the core `@context` declares `objectList` with `@container: "@list"` and
/// no `@type: "@id"` coercion, so that string expands to an `@value` node; a
/// context that does coerce, and every document written in expanded form, gives
/// an `@id` node instead. All three spell the same target, so all three are
/// read; a member that spells none is not a target at all and refuses the write
/// rather than silently shortening the list.
///
/// Shared with the clause 5.6.15 instance-replacement path in the temporal
/// store, so a `ListRelationship` written through an append and one written
/// through an instance PATCH cannot come to disagree about which spellings name
/// a target.
#[must_use]
pub fn object_list_target_iri(member: &Value) -> Option<&str> {
    member
        .get(expanded_properties::OBJECT.as_str())
        .and_then(Value::as_array)
        .and_then(|nodes| nodes.first())
        .and_then(|node| node.get("@id"))
        .or_else(|| member.get("@id"))
        .or_else(|| member.get("@value"))
        .and_then(Value::as_str)
}

/// Read one Relationship target, decoding the percent-encoding a client may
/// have written its entity identifier in.
///
/// A client is free to percent-encode characters in an entity identifier, and
/// the same identifier written two ways names one entity, so the decoding
/// happens once on the way in rather than at each presentation boundary that
/// would otherwise have to know the storage shape.
///
/// Shared with the clause 5.6.15 instance-replacement path in the temporal
/// store, so an appended target and a replaced one are stored identically.
///
/// # Errors
///
/// [`UriParseError`] when the decoded target is not a URI. The caller words the
/// refusal, because the member the target came from is what a client needs
/// named.
pub fn relationship_target_uri(raw: &str) -> Result<Uri, UriParseError> {
    decode_percent_encoded_uri(raw).parse::<Uri>()
}

/// Validate one Relationship target, naming the member it came from.
fn parse_target(raw: &str, context: &str) -> Result<Uri, TemporalError> {
    relationship_target_uri(raw).map_err(|error| TemporalError::validation(format!("{context} is not a valid URI: {error}")))
}

/// The JSON array of bare target URIs a multi-target Relationship family is
/// stored as.
fn uri_array(targets: &[Uri]) -> Value {
    Value::Array(targets.iter().map(|target| Value::String(target.to_string())).collect())
}

/// Decode a Property scalar, structured value, or typed temporal literal.
fn property_value(instance: &Value) -> Result<TemporalValue, TemporalError> {
    let nodes = instance
        .get(expanded_properties::VALUE.as_str())
        .and_then(Value::as_array)
        .ok_or_else(|| TemporalError::validation("Property missing value"))?;
    // Clause 4.9 makes an array-valued Property's target value the whole array
    // ("is included in the target value, if the latter is an array"), and
    // JSON-LD expands `"value": ["a","b","c"]` into one value node per element.
    // Keeping only the first would answer that rule against a single element and
    // drop the rest from the temporal evolution as well.
    if nodes.len() > 1 {
        let values = nodes
            .iter()
            .map(|node| {
                node.get("@value")
                    .cloned() // clone: the intent owns the element the expanded node holds.
                    .ok_or_else(|| TemporalError::validation("Property array element carries no value"))
            })
            .collect::<Result<Vec<Value>, TemporalError>>()?;
        return Ok(TemporalValue::Object(Value::Array(values)));
    }
    let value_node = first_member(instance, expanded_properties::VALUE.as_str()).ok_or_else(|| TemporalError::validation("Property missing value"))?;
    let Some(value) = value_node.get("@value") else {
        return Ok(TemporalValue::Object(Value::clone(value_node))); // clone: the intent owns the expanded node.
    };
    let type_hint = value_node.get("@type").and_then(Value::as_str);
    if let Some(raw) = value.as_str()
        && let Some(typed) = temporal_property_value(type_hint, raw)
    {
        return typed;
    }
    match value {
        // The number keeps the shape it was parsed in all the way to the
        // arbitrary-precision storage column: flattening it to a double here
        // would round an integer beyond 2^53 before anything could refuse it.
        Value::Number(number) => JsonNumber::try_from(number)
            .map(TemporalValue::Number)
            .map_err(|error| TemporalError::validation(format!("Property number cannot be carried: {error}"))),
        Value::String(text) => Ok(TemporalValue::String(text.clone())), // clone: the intent owns the text.
        Value::Bool(flag) => Ok(TemporalValue::Boolean(*flag)),
        Value::Array(_) | Value::Object(_) => Ok(TemporalValue::Object(Value::clone(value))), // clone: the intent owns the structured value.
        Value::Null => Ok(TemporalValue::String("null".to_owned())),
    }
}

/// Decode a Property value the client typed as one of the three clause 4.6.3
/// temporal forms, or `None` when the declared type is not one of them.
///
/// Each form is parsed rather than taken verbatim, and the two that are stored as
/// text are stored in the canonical rendering their type defines. Clause 4.6.3
/// admits many spellings of one `Time` — any offset, any fractional width — and
/// clause 4.9 orders `Time` values chronologically; the string column those values
/// land in orders them byte-wise, so only a fixed-width UTC rendering makes the
/// two orders the same. `Date` is validated for the same reason: its canonical
/// `YYYY-MM-DD` form already sorts chronologically, and a lenient spelling would
/// not. `DateTime` is routed through [`NgsiLdDateTime`] rather than a bare
/// `chrono` RFC 3339 parse for the same reason `observedAt` is: a non-`Z` offset
/// or more than six fractional digits must be refused rather than silently
/// reinterpreted into the `timestamptz` column, on every axis that carries a
/// client-supplied instant.
fn temporal_property_value(type_hint: Option<&str>, raw: &str) -> Option<Result<TemporalValue, TemporalError>> {
    let hint = type_hint?;
    if hint == expanded_classes::DATE_TIME.as_str() {
        return Some(
            raw.parse::<NgsiLdDateTime>()
                .map(NgsiLdDateTime::get)
                .map(TemporalValue::DateTime)
                .map_err(|error| TemporalError::validation(format!("invalid datetime property '{raw}': {error}"))),
        );
    }
    if hint == expanded_classes::TIME.as_str() {
        return Some(
            raw.parse::<NgsiLdTime>()
                .map(|time| TemporalValue::String(time.to_string()))
                .map_err(|error| TemporalError::validation(format!("invalid Time property '{raw}': {error}"))),
        );
    }
    if hint == expanded_classes::DATE.as_str() {
        return Some(
            raw.parse::<NgsiLdDate>()
                .map(|date| TemporalValue::String(date.to_string()))
                .map_err(|error| TemporalError::validation(format!("invalid Date property '{raw}': {error}"))),
        );
    }
    None
}

/// Borrow the first value of one expanded array member.
fn first_member<'a>(instance: &'a Value, key: &str) -> Option<&'a Value> {
    instance.get(key).and_then(Value::as_array).and_then(|values| values.first())
}

#[cfg(test)]
mod tests {
    use crate::attribute_instance_value::{extract_temporal_value, object_list_target_iri};
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use andromeda_ngsi_ld_model::temporal::TemporalValue;
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use serde_json::{Value, json};

    /// One expanded Relationship instance pointing at `targets`.
    fn relationship(targets: &[&str]) -> Value {
        let nodes: Vec<Value> = targets.iter().map(|target| json!({"@id": target})).collect();
        json!({expanded_properties::OBJECT.as_str(): nodes})
    }

    /// Clause 4.5.3.2: `object` is "a URI or array of URIs", so a single target
    /// keeps the URI shape.
    #[test]
    fn a_single_target_relationship_is_decoded_as_one_uri() {
        let value = extract_temporal_value(&relationship(&["urn:ngsi-ld:Person:Alice"]), AttributeType::Relationship).expect("a valid Relationship");
        pretty_assertions::assert_eq!(value, TemporalValue::Uri("urn:ngsi-ld:Person:Alice".parse().expect("a valid URI")));
    }

    /// Clause 4.5.3.2 and clause 4.5.9 EXAMPLE 7: every target of a
    /// multi-target Relationship survives into the temporal evolution, in the
    /// order the instance declared them.
    #[test]
    fn every_target_of_a_multi_target_relationship_survives_decoding() {
        let value = extract_temporal_value(
            &relationship(&["urn:ngsi-ld:Device:14142", "urn:ngsi-ld:Device:13562", "urn:ngsi-ld:Device:37309"]),
            AttributeType::Relationship,
        )
        .expect("a valid Relationship");
        pretty_assertions::assert_eq!(
            value,
            TemporalValue::Object(json!(["urn:ngsi-ld:Device:14142", "urn:ngsi-ld:Device:13562", "urn:ngsi-ld:Device:37309"]))
        );
    }

    /// A Relationship with no object at all is refused rather than stored with
    /// an empty target list.
    #[test]
    fn a_relationship_without_any_object_is_refused() {
        let error = extract_temporal_value(&relationship(&[]), AttributeType::Relationship).expect_err("no target");
        assert!(error.to_string().contains("object"), "{error}");
    }

    /// Clauses 4.5.22.2 and 4.5.22.3: the normalized `{"object": …}` member,
    /// the concise bare URI and the coerced `@id` node all name one target.
    #[test]
    fn every_conformant_object_list_member_spelling_names_its_target() {
        let normalized = json!({expanded_properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:City:Antwerp"}]});
        let concise = json!({"@value": "urn:ngsi-ld:City:Antwerp"});
        let coerced = json!({"@id": "urn:ngsi-ld:City:Antwerp"});
        for member in [normalized, concise, coerced] {
            pretty_assertions::assert_eq!(object_list_target_iri(&member), Some("urn:ngsi-ld:City:Antwerp"), "{member}");
        }
        pretty_assertions::assert_eq!(object_list_target_iri(&json!({"nothing": "here"})), None);
    }

    /// Clause 4.5.22.2: the normalized `objectList` a client writes to a
    /// temporal endpoint is stored as the ordered array of targets it names.
    #[test]
    fn a_normalized_object_list_is_decoded_into_its_ordered_targets() {
        let instance = json!({
            expanded_properties::OBJECT_LIST.as_str(): [{"@list": [
                {expanded_properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:City:Antwerp"}]},
                {expanded_properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:City:Rotterdam"}]},
            ]}]
        });
        let value = extract_temporal_value(&instance, AttributeType::ListRelationship).expect("a valid ListRelationship");
        pretty_assertions::assert_eq!(value, TemporalValue::Object(json!(["urn:ngsi-ld:City:Antwerp", "urn:ngsi-ld:City:Rotterdam"])));
    }

    /// A percent-encoded entity identifier is decoded once, on the way in, so
    /// the stored target is the identifier the Relationship names.
    #[test]
    fn a_percent_encoded_target_is_decoded_before_it_is_stored() {
        let value = extract_temporal_value(&relationship(&["urn%3Angsi-ld%3APerson%3AAlice"]), AttributeType::Relationship).expect("a valid Relationship");
        pretty_assertions::assert_eq!(value, TemporalValue::Uri("urn:ngsi-ld:Person:Alice".parse().expect("a valid URI")));
    }

    /// A member that names no target refuses the write rather than shortening
    /// the list.
    #[test]
    fn an_object_list_member_naming_no_target_refuses_the_write() {
        let instance = json!({
            expanded_properties::OBJECT_LIST.as_str(): [{"@list": [{"@value": "urn:ngsi-ld:City:Antwerp"}, {"unrelated": 1}]}]
        });
        let error = extract_temporal_value(&instance, AttributeType::ListRelationship).expect_err("a member with no target");
        assert!(error.to_string().contains("target URI"), "{error}");
    }
}
