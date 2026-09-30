//! The temporal value one expanded NGSI-LD attribute instance carries, and the
//! metadata that travels beside it.
//!
//! One decoder per attribute family (clause 4.5.1 and the per-family clauses
//! 4.5.2, 4.5.3, 4.5.18, 4.5.20, 4.5.21, 4.5.22 and 4.5.24), because the member
//! a family's value lives under and the JSON-LD keyword that member's nodes
//! carry are both family-decided. Everything the family does not name as its
//! value member is metadata and is kept verbatim.

use crate::{error::TemporalError, temporal_invariant::TemporalInvariant};
use andromeda_context_commit::submission_refusal::{RelationshipTargetMember, SubmissionRefusal};
use andromeda_ngsi_ld_lexicon::{
    attribute_type::AttributeType,
    time_value::TimeValue,
    uri::{Uri, UriParseError},
};
use andromeda_ngsi_ld_model::{expanded::geo_node::expanded_geo_node_to_geometry, temporal::TemporalValue};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_render::uri_decoding::decode_percent_encoded_uri;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
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
        // Clause 4.8: an instance's createdAt/modifiedAt are stamped into the
        // current-state payload and carried on the temporal row's own columns
        // (from the write clock); keeping them out of the metadata blob stops
        // the observation from echoing a current-state system time back as a
        // dated sub-Property of a past observation.
        || key == expanded_properties::CREATED_AT.as_str()
        || key == expanded_properties::MODIFIED_AT.as_str()
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
        AttributeType::LanguageProperty => owned_member(
            instance,
            expanded_properties::LANGUAGE_MAP.as_str(),
            SubmissionRefusal::LanguagePropertyMissingLanguageMap,
        ),
        AttributeType::JsonProperty => nested_value(instance, expanded_properties::JSON.as_str(), SubmissionRefusal::JsonPropertyMissingJson),
        AttributeType::ListProperty => list_value(
            instance,
            expanded_properties::VALUE_LIST.as_str(),
            SubmissionRefusal::ListPropertyMissingValueList,
        ),
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
        .ok_or_else(|| TemporalError::from(SubmissionRefusal::RelationshipMissingObject))?;
    let mut targets = nodes
        .iter()
        .map(|node| {
            let raw = node
                .get("@id")
                .and_then(Value::as_str)
                .ok_or_else(|| TemporalError::from(SubmissionRefusal::RelationshipObjectNoTarget))?;
            parse_target(raw, RelationshipTargetMember::RelationshipObject)
        })
        .collect::<Result<Vec<Uri>, TemporalError>>()?;
    match targets.pop() {
        Some(single) if targets.is_empty() => Ok(TemporalValue::Uri(single)),
        Some(last) => {
            targets.push(last);
            Ok(TemporalValue::Object(uri_array(&targets)))
        }
        None => Err(TemporalError::from(SubmissionRefusal::RelationshipMissingObject)),
    }
}

/// Decode an expanded or encoded `GeoProperty` value.
fn geometry_value(instance: &Value) -> Result<TemporalValue, TemporalError> {
    let node = first_member(instance, expanded_properties::VALUE.as_str()).ok_or_else(|| TemporalError::from(SubmissionRefusal::GeoPropertyMissingValue))?;
    let geometry = if let Some(value) = node.get("@value") {
        match value.as_str() {
            Some(raw) => serde_json::from_str(raw).map_err(|source| TemporalError::from(SubmissionRefusal::GeoPropertyGeoJson(source)))?,
            None => Value::clone(value), // clone: the intent owns the geometry value.
        }
    } else {
        let geometry = expanded_geo_node_to_geometry(node).ok_or_else(|| TemporalError::from(SubmissionRefusal::GeoPropertyGeometryInvalid))?;
        serde_json::to_value(geometry.to_geojson()).map_err(|error| TemporalError::from(TemporalInvariant::GeometrySerialization(error)))?
    };
    Ok(TemporalValue::Geometry(geometry))
}

/// Decode one or many `VocabProperty` identifiers.
fn vocab_value(instance: &Value) -> Result<TemporalValue, TemporalError> {
    let values = instance
        .get(expanded_properties::VOCAB.as_str())
        .and_then(Value::as_array)
        .ok_or_else(|| TemporalError::from(SubmissionRefusal::VocabPropertyMissingVocab))?;
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
            .ok_or_else(|| TemporalError::from(SubmissionRefusal::VocabPropertyMissingVocab))
    }
}

/// Own an expanded member used directly as a temporal object value.
fn owned_member(instance: &Value, key: &str, reason: SubmissionRefusal) -> Result<TemporalValue, TemporalError> {
    instance
        .get(key)
        .cloned()
        .map(TemporalValue::Object)
        .ok_or_else(move || TemporalError::from(reason))
}

/// Own the `@value` nested in the first expanded member.
fn nested_value(instance: &Value, key: &str, reason: SubmissionRefusal) -> Result<TemporalValue, TemporalError> {
    first_member(instance, key)
        .and_then(|value| value.get("@value"))
        .cloned()
        .map(TemporalValue::Object)
        .ok_or_else(move || TemporalError::from(reason))
}

/// Own the `@list` nested in the first expanded member.
fn list_value(instance: &Value, key: &str, reason: SubmissionRefusal) -> Result<TemporalValue, TemporalError> {
    first_member(instance, key)
        .and_then(|value| value.get("@list"))
        .cloned()
        .map(TemporalValue::Object)
        .ok_or_else(move || TemporalError::from(reason))
}

/// Decode a `ListRelationship` object list.
fn list_relationship_value(instance: &Value) -> Result<TemporalValue, TemporalError> {
    let list = first_member(instance, expanded_properties::OBJECT_LIST.as_str())
        .and_then(|value| value.get("@list"))
        .and_then(Value::as_array)
        .ok_or_else(|| TemporalError::from(SubmissionRefusal::ListRelationshipMissingObjectList))?;
    let targets = list
        .iter()
        .map(|member| {
            let raw = object_list_target_iri(member).ok_or_else(|| TemporalError::from(SubmissionRefusal::ListRelationshipObjectListMemberNoTarget))?;
            parse_target(raw, RelationshipTargetMember::ListRelationshipObjectListMember)
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
fn parse_target(raw: &str, member: RelationshipTargetMember) -> Result<Uri, TemporalError> {
    relationship_target_uri(raw).map_err(|source| TemporalError::from(SubmissionRefusal::RelationshipTarget { member, source }))
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
        .ok_or_else(|| TemporalError::from(SubmissionRefusal::PropertyMissingValue))?;
    // Clause 4.9 makes an array-valued Property's target value the whole array
    // ("is included in the target value, if the latter is an array"), and
    // JSON-LD expands `"value": ["a","b","c"]` into one value node per element.
    // Keeping only the first would answer that rule against a single element and
    // drop the rest from the temporal evolution as well.
    if nodes.len() > 1 {
        let values = nodes.iter().map(property_array_element).collect::<Result<Vec<Value>, TemporalError>>()?;
        return Ok(TemporalValue::Object(Value::Array(values)));
    }
    let value_node = first_member(instance, expanded_properties::VALUE.as_str()).ok_or_else(|| TemporalError::from(SubmissionRefusal::PropertyMissingValue))?;
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
            .map_err(|source| TemporalError::from(SubmissionRefusal::PropertyNumber(source))),
        Value::String(text) => Ok(TemporalValue::String(text.clone())), // clone: the intent owns the text.
        Value::Bool(flag) => Ok(TemporalValue::Boolean(*flag)),
        Value::Array(_) | Value::Object(_) => Ok(TemporalValue::Object(Value::clone(value))), // clone: the intent owns the structured value.
        Value::Null => Ok(TemporalValue::String("null".to_owned())),
    }
}

/// The stored form of one element of an array-valued Property.
///
/// Clause 5.2.5 puts no restriction on a Property `value`, so an array may hold
/// JSON objects beside scalars, and JSON-LD expansion turns each object into a
/// node object rather than a value node. A value node holding a scalar is kept
/// as that scalar, which is the shape an all-scalar array has always been stored
/// in; every other node -- a node object, a typed `@json` literal, a `@list` --
/// is kept as the expanded node it is, so its members compact back to the keys
/// the client wrote. The two shapes cannot be confused on the way back: a stored
/// element that is a JSON object is always a node, because a scalar value node
/// is stored as the bare scalar.
///
/// # Errors
///
/// [`TemporalError::Validation`] when the expanded element is not a JSON-LD
/// node at all, which no conformant expansion produces.
fn property_array_element(node: &Value) -> Result<Value, TemporalError> {
    let object = node
        .as_object()
        .ok_or_else(|| TemporalError::from(SubmissionRefusal::PropertyArrayElementMissingValue))?;
    let json_literal = object.get("@type").and_then(Value::as_str) == Some("@json");
    let scalar = match object.get("@value") {
        Some(value @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_))) if !json_literal => Some(value),
        Some(Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) | Value::Object(_)) | None => None,
    };
    // clone: the intent owns the element the expanded node holds.
    Ok(scalar.map_or_else(|| node.clone(), Value::clone))
}

/// Decode a Property value that is one of the three clause 4.6.3 time values, or
/// `None` when it is an ordinary JSON string.
///
/// The classification is [`TimeValue::admitted`]'s and nothing here repeats it:
/// a `@type` the client declared names the form, and with none declared the
/// clause 4.6.3 lexical form of the literal does — "`DateTime` string for encoding
/// a timestamp" is a data type whose whole representation is the string, and
/// clause 4.8 NOTE 3 puts such a value on an ordinary Property. Sharing that one
/// classifier with the `q` operand parser is what makes clause 4.9's rule that
/// "if there is no equality between the target value data type and the Query
/// Term value data type, then it shall be considered as not matching" a rule
/// about the values rather than about which side of the broker read them.
///
/// Each form is parsed rather than taken verbatim, and the two that are stored
/// as text are stored in the canonical rendering their type defines. Clause
/// 4.6.3 admits many spellings of one `Time` — any fractional width, and a comma
/// separator in requests — and clause 4.9 orders `Time` values chronologically;
/// the string column those values land in orders them byte-wise, so only a
/// fixed-width UTC rendering makes the two orders the same. `Date` is
/// canonicalised for the same reason: its `YYYY-MM-DD` form already sorts
/// chronologically, and a lenient spelling would not. A `DateTime` reaches its
/// own `timestamptz` column, where the instant, not its spelling, is what is
/// compared.
fn temporal_property_value(type_hint: Option<&str>, raw: &str) -> Option<Result<TemporalValue, TemporalError>> {
    TimeValue::admitted(type_hint, raw).map(|admitted| {
        admitted
            .map(recorded_time_value)
            .map_err(|source| TemporalError::from(SubmissionRefusal::TimeValueProperty(source)))
    })
}

/// The neutral observation one clause 4.6.3 time value is recorded as.
fn recorded_time_value(value: TimeValue) -> TemporalValue {
    match value {
        TimeValue::DateTime(instant) => TemporalValue::DateTime(instant.get()),
        TimeValue::Date(date) => TemporalValue::String(date.to_string()),
        TimeValue::Time(time) => TemporalValue::String(time.to_string()),
    }
}

/// Borrow the first value of one expanded array member.
fn first_member<'a>(instance: &'a Value, key: &str) -> Option<&'a Value> {
    instance.get(key).and_then(Value::as_array).and_then(|values| values.first())
}

#[cfg(test)]
mod tests {
    use crate::attribute_instance_value::{extract_temporal_value, object_list_target_iri};
    use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, ngsi_ld_date::NgsiLdDate, ngsi_ld_time::NgsiLdTime};
    use andromeda_ngsi_ld_model::temporal::TemporalValue;
    use andromeda_ngsi_ld_query::filter::{quoting::Quoting, scalar::Scalar};
    use andromeda_ngsi_ld_vocabulary::expanded::{classes as expanded_classes, properties as expanded_properties};
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

    /// One expanded Property instance whose value node carries a bare literal,
    /// which is what JSON-LD expansion of `"value": "…"` produces.
    fn bare_property(literal: &str) -> Value {
        json!({expanded_properties::VALUE.as_str(): [{"@value": literal}]})
    }

    /// The value one bare-literal Property is recorded as.
    fn recorded(literal: &str) -> TemporalValue {
        extract_temporal_value(&bare_property(literal), AttributeType::Property).expect("a Property value")
    }

    /// Clause 4.6.3 lists a "`DateTime` string for encoding a timestamp" among the
    /// data types a Value may have, and clause 4.8 NOTE 3 puts such a value on an
    /// ordinary Property. Its whole representation is the string, so the string
    /// is what says it is one — and until it did, a client's timestamp reached
    /// the string column and no `dateTime` operand could ever find it.
    #[test]
    fn a_bare_timestamp_literal_is_recorded_as_the_instant_it_names() {
        let recorded = recorded("2024-01-01T00:00:00.250000Z");
        let TemporalValue::DateTime(instant) = recorded else {
            panic!("a conformant DateTime literal is an instant, got {recorded:?}");
        };
        pretty_assertions::assert_eq!(instant.to_rfc3339_opts(chrono::SecondsFormat::Micros, true), "2024-01-01T00:00:00.250000Z");
    }

    /// Clause 4.9 orders `Time` and `Date` values chronologically and the string
    /// column they land in orders byte-wise, so each is recorded in the one
    /// fixed-width canonical rendering its lexical type defines.
    #[test]
    fn a_bare_time_or_date_literal_is_recorded_in_its_canonical_rendering() {
        pretty_assertions::assert_eq!(recorded("12:00:00Z"), TemporalValue::String("12:00:00.000000Z".to_owned()));
        pretty_assertions::assert_eq!(recorded("12:00:00,5Z"), TemporalValue::String("12:00:00.500000Z".to_owned()));
        pretty_assertions::assert_eq!(recorded("2024-01-01"), TemporalValue::String("2024-01-01".to_owned()));
    }

    /// Clause 4.6.3 admits "all the JSON native data types" beside the three
    /// time value forms, so a string that only resembles one — a numeric offset
    /// where the clause demands `Z`, a seventh fractional digit, free-form text
    /// — is the string it is written as rather than a refused write.
    #[test]
    fn a_literal_that_only_resembles_a_time_value_stays_the_string_it_is() {
        for literal in [
            "2024-01-01T01:00:00+01:00",
            "2024-01-01T00:00:00.1234567Z",
            "13:00:00+01:00",
            "12:00:00 in the morning",
            "atrium",
        ] {
            pretty_assertions::assert_eq!(recorded(literal), TemporalValue::String(literal.to_owned()), "{literal}");
        }
    }

    /// A `@type` the client declared still overrides the literal's own form and
    /// still refuses a literal that does not satisfy it (annex C.6).
    #[test]
    fn a_declared_type_still_refuses_a_literal_that_does_not_satisfy_it() {
        let instance = json!({
            expanded_properties::VALUE.as_str(): [{"@value": "13:00:00+01:00", "@type": expanded_classes::TIME.as_str()}]
        });
        let error = extract_temporal_value(&instance, AttributeType::Property).expect_err("a numeric offset is forbidden on a Time");
        assert!(error.to_string().contains("time value property"), "{error}");
    }

    /// Which clause 4.6.3 data type a literal has, read by whichever side of a
    /// comparison read it.
    #[derive(Debug, PartialEq, Eq)]
    enum ReadAs {
        /// The literal is a `DateTime`.
        DateTime,
        /// The literal is a `Date`.
        Date,
        /// The literal is a `Time`.
        Time,
        /// The literal is an ordinary JSON string.
        PlainString,
        /// The literal is a malformed operand and the request is refused.
        ///
        /// Only the query side can reach this: a stored value is a document the
        /// broker keeps, while an operand is a Query Term the clause 4.9 grammar
        /// either derives or does not.
        Refused,
    }

    /// How the write path reads one bare literal.
    ///
    /// A `Date` and a `Time` share the string column with an ordinary string, so
    /// the canonical rendering of the value is what tells them apart — which is
    /// exactly what the `q` compiler binds against that column.
    fn written_as(literal: &str) -> ReadAs {
        match recorded(literal) {
            TemporalValue::DateTime(_) => ReadAs::DateTime,
            TemporalValue::String(text) => {
                if text.parse::<NgsiLdDate>().is_ok_and(|date| date.to_string() == text) {
                    ReadAs::Date
                } else if text.parse::<NgsiLdTime>().is_ok_and(|time| time.to_string() == text) {
                    ReadAs::Time
                } else {
                    ReadAs::PlainString
                }
            }
            other @ (TemporalValue::Number(_)
            | TemporalValue::Boolean(_)
            | TemporalValue::Geometry(_)
            | TemporalValue::Object(_)
            | TemporalValue::Uri(_)
            | TemporalValue::Scopes(_)
            | TemporalValue::Tombstone { .. }) => panic!("a string literal is not {other:?}"),
        }
    }

    /// How the clause 4.9 operand parser reads the same literal in one spelling.
    fn queried_as(literal: &str, quoting: Quoting) -> ReadAs {
        match Scalar::from_literal(literal, quoting) {
            Ok(Scalar::DateTime(_)) => ReadAs::DateTime,
            Ok(Scalar::Date(_)) => ReadAs::Date,
            Ok(Scalar::Time(_)) => ReadAs::Time,
            Ok(Scalar::Str(_)) => ReadAs::PlainString,
            Ok(other @ (Scalar::Number(_) | Scalar::Bool(_))) => panic!("a string literal is not {other:?}"),
            // A refusal is its own outcome. Folding it into `PlainString` let
            // this table pass while the two sides disagreed about the literal.
            Err(_) => ReadAs::Refused,
        }
    }

    /// Clause 4.9's `equal`: "If there is no equality between the target value
    /// data type and the Query Term value data type, then it shall be considered
    /// as not matching." Two classifiers can only satisfy that sentence by
    /// never matching, which is precisely what a stored timestamp read as a
    /// String and an operand read as a `dateTime` did. One classifier reads
    /// both sides, and this pins that they land in the same place.
    ///
    /// The quoted operand is the one that has to agree with the write path,
    /// because clause 4.9's `quotedStr` production carries its quotes and so
    /// derives any literal at all. A bare literal written in a clause 4.6.3 form
    /// it does not satisfy is derivable from no production of that grammar, and
    /// a request the grammar cannot derive is refused rather than compared.
    #[test]
    fn the_write_side_and_the_query_side_read_every_literal_the_same_way() {
        for (literal, written, bare, quoted) in [
            ("2024-01-01T00:00:00Z", ReadAs::DateTime, ReadAs::DateTime, ReadAs::DateTime),
            ("2024-01-01T00:00:00.250000Z", ReadAs::DateTime, ReadAs::DateTime, ReadAs::DateTime),
            ("2024-01-01T00:00:00,25Z", ReadAs::DateTime, ReadAs::DateTime, ReadAs::DateTime),
            ("2024-01-01", ReadAs::Date, ReadAs::Date, ReadAs::Date),
            ("12:00:00Z", ReadAs::Time, ReadAs::Time, ReadAs::Time),
            ("12:00:00.5Z", ReadAs::Time, ReadAs::Time, ReadAs::Time),
            ("12:00:00,5Z", ReadAs::Time, ReadAs::Time, ReadAs::Time),
            ("2024-01-01T01:00:00+01:00", ReadAs::PlainString, ReadAs::Refused, ReadAs::PlainString),
            ("13:00:00+01:00", ReadAs::PlainString, ReadAs::Refused, ReadAs::PlainString),
            ("12:00:00 in the morning", ReadAs::PlainString, ReadAs::PlainString, ReadAs::PlainString),
            ("urn:ngsi-ld:Vehicle:A4567", ReadAs::PlainString, ReadAs::PlainString, ReadAs::PlainString),
            ("atrium", ReadAs::PlainString, ReadAs::PlainString, ReadAs::PlainString),
        ] {
            pretty_assertions::assert_eq!(written_as(literal), written, "the write path reads {literal}");
            pretty_assertions::assert_eq!(queried_as(literal, Quoting::Quoted), quoted, "a quoted operand reads {literal}");
            pretty_assertions::assert_eq!(queried_as(literal, Quoting::Bare), bare, "a bare operand reads {literal}");
        }
    }

    /// Clause 5.2.5 puts no restriction on a Property `value`: an array holding
    /// several JSON objects expands to several node objects, and each is kept as
    /// the node it is rather than refusing the write.
    #[test]
    fn an_array_of_several_objects_is_kept_node_by_node() {
        let first = json!({"https://example.org/zone": [{"@value": "Europe/Andorra"}]});
        let second = json!({"https://example.org/zone": [{"@value": "Asia/Dubai"}]});
        let instance = json!({expanded_properties::VALUE.as_str(): [first, second]});
        let value = extract_temporal_value(&instance, AttributeType::Property).expect("an array of objects is a Property value");
        pretty_assertions::assert_eq!(
            value,
            TemporalValue::Object(json!([
                {"https://example.org/zone": [{"@value": "Europe/Andorra"}]},
                {"https://example.org/zone": [{"@value": "Asia/Dubai"}]}
            ]))
        );
    }

    /// A scalar beside an object keeps the bare-scalar shape an all-scalar array
    /// is stored in, and the object keeps its node.
    #[test]
    fn a_scalar_beside_an_object_keeps_its_bare_shape() {
        let node = json!({"https://example.org/a": [{"@value": 1}]});
        let instance = json!({expanded_properties::VALUE.as_str(): [{"@value": 1}, node]});
        let value = extract_temporal_value(&instance, AttributeType::Property).expect("a mixed array is a Property value");
        pretty_assertions::assert_eq!(value, TemporalValue::Object(json!([1, {"https://example.org/a": [{"@value": 1}]}])));
    }

    /// An all-scalar array is stored exactly as before: one bare scalar per
    /// value node, whatever `@type` a context coerced onto the element.
    #[test]
    fn an_all_scalar_array_is_stored_as_bare_scalars() {
        let instance = json!({expanded_properties::VALUE.as_str(): [
            {"@value": "a"},
            {"@value": "2024-01-01T00:00:00Z", "@type": expanded_classes::DATE_TIME.as_str()},
            {"@value": true}
        ]});
        let value = extract_temporal_value(&instance, AttributeType::Property).expect("a scalar array");
        pretty_assertions::assert_eq!(value, TemporalValue::Object(json!(["a", "2024-01-01T00:00:00Z", true])));
    }

    /// A typed `@json` literal inside an array is a node, not a scalar, so it
    /// is kept whole and renders back as the literal it was.
    #[test]
    fn a_json_literal_element_is_kept_as_its_node() {
        let literal = json!({"@value": {"k": [1, 2]}, "@type": "@json"});
        let instance = json!({expanded_properties::VALUE.as_str(): [{"@value": 3}, literal]});
        let value = extract_temporal_value(&instance, AttributeType::Property).expect("a mixed array");
        pretty_assertions::assert_eq!(value, TemporalValue::Object(json!([3, {"@value": {"k": [1, 2]}, "@type": "@json"}])));
    }

    /// An element that is not a JSON-LD node at all is refused.
    #[test]
    fn an_element_that_is_no_node_is_refused() {
        let instance = json!({expanded_properties::VALUE.as_str(): [{"@value": 1}, "bare"]});
        let error = extract_temporal_value(&instance, AttributeType::Property).expect_err("a bare string is no expanded node");
        assert!(error.to_string().contains("carries no value"), "{error}");
    }

    /// The text a `Time` operand binds against the string column has to be the
    /// text the write path put there, or the two agree on the data type and
    /// still never match.
    #[test]
    fn a_time_operand_binds_the_text_the_write_path_stored() {
        for literal in ["12:00:00Z", "12:00:00.5Z", "12:00:00,5Z", "12:00:00.500000Z"] {
            let Ok(Scalar::Time(operand)) = Scalar::from_literal(literal, Quoting::Bare) else {
                panic!("{literal} is a Time operand");
            };
            let bound = NgsiLdTime::normalise(operand).expect("a normalisable time of day").to_string();
            pretty_assertions::assert_eq!(recorded(literal), TemporalValue::String(bound), "{literal}");
        }
    }
}
