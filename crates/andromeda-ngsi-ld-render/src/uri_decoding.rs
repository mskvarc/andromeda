//! Percent-decoding of the entity identifiers Relationships target.
//!
//! An entity identifier is a URI, and a client is free to percent-encode
//! characters in one when it writes a Relationship. The store keeps what it was
//! given, so the decoding happens at the presentation boundary, and it reaches
//! exactly the members that hold a target: a Relationship's `object` and
//! `previousObject`, and a `ListRelationship`'s `objectList` and
//! `previousObjectList`, wherever the attribute instance stands -- at the top
//! of an entity, as a sub-attribute, or inside an entity an inline join nested
//! (clause 4.5.23.2). A client's own value -- a Property's `value`, a
//! `JsonProperty`'s `json` -- is never decoded, whatever it holds, and no
//! target is decoded twice.
//!
//! The typed walk of [`crate::entity_body`] applies the rule as it writes each
//! member, through [`UriDecoding`] and [`DecodedTargets`].
//! [`decode_percent_encoded_uris`] applies it to a payload that is still a
//! rendered document -- a temporal entity, a notification shaped as a document,
//! a member the broker carries verbatim -- where an instance is recognised by
//! the family its `type` names or, in the concise representation that drops
//! `type`, by the `object` or `objectList` member that identifies the family
//! (clauses 4.5.3.3 and 4.5.22.3). The simplified representation keeps no
//! member that tells a target from a value, so a document is decoded before it
//! is simplified.

use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, uri::Uri};
use andromeda_ngsi_ld_model::attribute::one_or_many::OneOrMany;
use andromeda_ngsi_ld_representation::entity_members::ENTITY_MEMBERS;
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, is_core_term};
use serde::{Serialize, Serializer};
use serde_json::{Map, Value};
use std::{borrow::Cow, str::FromStr};

/// Whether the presentation boundary percent-decodes Relationship targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UriDecoding {
    /// Every target is written decoded.
    Decode,
    /// Every target is written as the store holds it.
    Keep,
}

/// The targets of one Relationship member, written under a [`UriDecoding`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct DecodedTargets<'a> {
    /// The target or targets, as the Relationship spells them.
    targets: &'a OneOrMany<Uri>,
    /// Whether they are decoded.
    decoding: UriDecoding,
}

impl<'a> DecodedTargets<'a> {
    /// `targets`, written under `decoding`.
    pub(crate) const fn new(targets: &'a OneOrMany<Uri>, decoding: UriDecoding) -> DecodedTargets<'a> {
        DecodedTargets { targets, decoding }
    }
}

impl Serialize for DecodedTargets<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.targets {
            OneOrMany::One(target) => DecodedTarget::new(target, self.decoding).serialize(serializer),
            OneOrMany::Many(targets) => serializer.collect_seq(targets.iter().map(|target| DecodedTarget::new(target, self.decoding))),
        }
    }
}

/// One target, written under a [`UriDecoding`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct DecodedTarget<'a> {
    /// The target.
    target: &'a Uri,
    /// Whether it is decoded.
    decoding: UriDecoding,
}

impl<'a> DecodedTarget<'a> {
    /// `target`, written under `decoding`.
    pub(crate) const fn new(target: &'a Uri, decoding: UriDecoding) -> DecodedTarget<'a> {
        DecodedTarget { target, decoding }
    }
}

impl Serialize for DecodedTarget<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.decoding {
            UriDecoding::Decode => serializer.serialize_str(&decode_percent_encoded_uri(self.target.as_str())),
            UriDecoding::Keep => serializer.serialize_str(self.target.as_str()),
        }
    }
}

/// Decode the Relationship targets of a rendered entity, or of every entity of
/// a rendered collection.
///
/// It is `pub` because the notification worker shapes its documents outside
/// this crate and must decode by exactly this rule, so a query response and a
/// notification of the same entity carry the same targets.
pub fn decode_percent_encoded_uris(payload: &mut Value) {
    match payload {
        Value::Object(entity) => decode_entity(entity),
        Value::Array(entities) => entities.iter_mut().for_each(decode_entity_value),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Decode the targets of every attribute of one rendered entity.
fn decode_entity(entity: &mut Map<String, Value>) {
    entity
        .iter_mut()
        .filter(|(name, _)| !ENTITY_MEMBERS.contains(&name.as_str()))
        .for_each(|(_, member)| decode_attribute_targets(member));
}

/// Decode the targets of one rendered attribute member: one instance, or the
/// array of its instances.
fn decode_attribute_targets(member: &mut Value) {
    match member {
        Value::Object(instance) => decode_instance(instance),
        Value::Array(instances) => instances.iter_mut().filter_map(Value::as_object_mut).for_each(decode_instance),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Decode the targets one rendered attribute instance holds, those of its
/// sub-attributes, and those of the entities an inline join nested into it.
fn decode_instance(instance: &mut Map<String, Value>) {
    match instance_family(instance) {
        Some(AttributeType::Relationship) => {
            for member in [compact_properties::OBJECT, compact_properties::PREVIOUS_OBJECT] {
                instance.get_mut(member).into_iter().for_each(decode_targets);
            }
        }
        Some(AttributeType::ListRelationship) => {
            for member in [compact_properties::OBJECT_LIST, compact_properties::PREVIOUS_OBJECT_LIST] {
                if let Some(Value::Array(list)) = instance.get_mut(member) {
                    list.iter_mut().for_each(decode_object_list_member);
                }
            }
        }
        Some(
            AttributeType::Property
            | AttributeType::GeoProperty
            | AttributeType::VocabProperty
            | AttributeType::LanguageProperty
            | AttributeType::JsonProperty
            | AttributeType::ListProperty,
        )
        | None => {}
    }
    for (name, member) in instance.iter_mut() {
        if name == compact_properties::ENTITY || name == compact_properties::ENTITY_LIST {
            decode_percent_encoded_uris(member);
        } else if !is_core_term(name) {
            decode_attribute_targets(member);
        }
    }
}

/// The family of a rendered instance: the one its `type` names, or, in the
/// concise representation, the one its identifying member implies.
fn instance_family(instance: &Map<String, Value>) -> Option<AttributeType> {
    match instance.get("type").and_then(Value::as_str) {
        Some(declared) => AttributeType::from_str(declared).ok(),
        None if instance.contains_key(compact_properties::OBJECT) => Some(AttributeType::Relationship),
        None if instance.contains_key(compact_properties::OBJECT_LIST) => Some(AttributeType::ListRelationship),
        None => None,
    }
}

/// Decode the targets of one rendered entity, if the value is one.
fn decode_entity_value(entity: &mut Value) {
    if let Value::Object(entity) = entity {
        decode_entity(entity);
    }
}

/// Decode a Relationship's target or array of targets.
fn decode_targets(targets: &mut Value) {
    match targets {
        Value::String(target) => decode_uri_in_place(target),
        Value::Array(targets) => targets.iter_mut().for_each(|target| {
            if let Value::String(target) = target {
                decode_uri_in_place(target);
            }
        }),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => {}
    }
}

/// Decode the target one `objectList` member names, in either of the
/// representations clause 4.5.22 gives it.
///
/// Table 5.2.37-1: the normalized member "holds a JSON object containing a
/// single Attribute with a key called `object`", the concise one "holds a valid
/// URI".
fn decode_object_list_member(member: &mut Value) {
    match member {
        Value::String(target) => decode_uri_in_place(target),
        Value::Object(fields) => {
            if let Some(Value::String(target)) = fields.get_mut(compact_properties::OBJECT) {
                decode_uri_in_place(target);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) => {}
    }
}

/// Decode a single percent-encoded URI in place, leaving it unchanged when it
/// carries no `%` or does not decode to a URI.
fn decode_uri_in_place(uri: &mut String) {
    if let Cow::Owned(decoded) = decode_percent_encoded_uri(uri) {
        *uri = decoded;
    }
}

/// Decode one percent-encoded URI without altering an undecodable or unescaped value.
///
/// The rule undoes a client's encoding of a whole identifier, such as
/// `urn%3Angsi-ld%3ABuilding%3A1`, so it applies only when what it decodes to
/// is itself a URI (clause 4.6.2). An escape whose decoding is not a URI
/// character -- the `%20` of `urn:ngsi-ld:City:a%20b` -- is part of the
/// identifier as written, and decoding it would name a different entity, or
/// none at all.
///
/// Temporal intent preparation uses the same narrow rule before validating a
/// Relationship target for typed history storage. This keeps current-entity
/// storage unchanged while preventing its durable temporal consequence from
/// assigning different semantics to the same target.
#[must_use]
pub fn decode_percent_encoded_uri(uri: &str) -> Cow<'_, str> {
    if uri.contains('%')
        && let Ok(decoded) = percent_encoding::percent_decode_str(uri).decode_utf8()
        && decoded.parse::<Uri>().is_ok()
    {
        return decoded;
    }
    Cow::Borrowed(uri)
}

#[cfg(test)]
mod tests {
    use crate::uri_decoding::{DecodedTargets, UriDecoding, decode_percent_encoded_uri, decode_percent_encoded_uris};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::attribute::one_or_many::OneOrMany;
    use serde_json::json;
    use std::borrow::Cow;

    fn uri(text: &str) -> Uri {
        text.parse().expect("a URI")
    }

    #[test]
    fn decode_percent_encoded_relationship_object() {
        let mut value = json!({
            "id": "urn:x", "type": "T",
            "ref": {"type": "Relationship", "object": "urn%3Angsi-ld%3ABuilding%3A1"}
        });
        decode_percent_encoded_uris(&mut value);
        pretty_assertions::assert_eq!(value["ref"]["object"], "urn:ngsi-ld:Building:1");
    }

    #[test]
    fn single_uri_decoding_borrows_plain_input_and_owns_decoded_input() {
        assert!(matches!(decode_percent_encoded_uri("urn:ngsi-ld:Building:1"), Cow::Borrowed(_)));
        pretty_assertions::assert_eq!(decode_percent_encoded_uri("urn%3Angsi-ld%3ABuilding%3A1"), "urn:ngsi-ld:Building:1");
    }

    /// Decoding `%20` would turn a valid identifier into a string no URI
    /// grammar admits, so the escape is kept as the client wrote it.
    #[test]
    fn an_escape_that_decodes_to_a_non_uri_character_is_kept() {
        for raw in ["urn:ngsi-ld:City:a%20b", "https://example.com/a%20b", "urn:ngsi-ld:City:a%0Ab"] {
            assert!(matches!(decode_percent_encoded_uri(raw), Cow::Borrowed(kept) if kept == raw), "{raw}");
        }
        let mut value = json!({"id": "urn:x", "type": "T", "ref": {"type": "Relationship", "object": "urn:ngsi-ld:City:a%20b"}});
        decode_percent_encoded_uris(&mut value);
        pretty_assertions::assert_eq!(value["ref"]["object"], "urn:ngsi-ld:City:a%20b");
    }

    /// A Property's value is the client's own: a `%` in it is a literal, and a
    /// value shaped like a Relationship object is still only a value.
    #[test]
    fn a_client_value_is_never_decoded_whatever_it_holds() {
        let mut value = json!({
            "id": "urn:x", "type": "T",
            "ref": {"type": "Relationship", "object": "urn%3Ay"},
            "ratio": {"type": "Property", "value": "50%"},
            "encoded": {"type": "Property", "value": "a%20b"},
            "shaped": {"type": "Property", "value": {"type": "Relationship", "object": "urn%3Az"}},
            "document": {"type": "JsonProperty", "json": {"object": "urn%3Aw"}}
        });
        let original = value.clone(); // clone: the expectation keeps the payload as written
        decode_percent_encoded_uris(&mut value);
        pretty_assertions::assert_eq!(value["ref"]["object"], "urn:y", "the relationship target is decoded");
        for member in ["ratio", "encoded", "shaped", "document"] {
            pretty_assertions::assert_eq!(value[member], original[member], "{member} is a client value");
        }
    }

    /// The decoding follows the structure of the entity, not the type it
    /// declares: an entity typed `Relationship` is still an entity, and its
    /// simplified members are values.
    #[test]
    fn an_entity_typed_like_a_relationship_family_keeps_its_values() {
        let mut value = json!({"id": "urn:x", "type": "Relationship", "object": "urn%3Ay", "objectList": ["urn%3Az"]});
        let original = value.clone(); // clone: the expectation keeps the payload as written
        decode_percent_encoded_uris(&mut value);
        pretty_assertions::assert_eq!(value, original);
    }

    /// Table 5.2.37-1: a normalized `objectList` member "holds a JSON object
    /// containing a single Attribute with a key called `object`", so the target
    /// it names is decoded exactly as the concise form's bare string is, and
    /// once.
    #[test]
    fn both_object_list_representations_have_their_targets_decoded_once() {
        let mut value = json!({
            "id": "urn:x", "type": "T",
            "normalized": {"type": "ListRelationship", "objectList": [{"object": "urn%3Aa"}, {"object": "urn:b:x%253A1"}]},
            "concise": {"objectList": ["urn%3Ac"]}
        });
        decode_percent_encoded_uris(&mut value);
        pretty_assertions::assert_eq!(value["normalized"]["objectList"], json!([{"object": "urn:a"}, {"object": "urn:b:x%3A1"}]));
        pretty_assertions::assert_eq!(value["concise"]["objectList"], json!(["urn:c"]));
    }

    /// Every target is decoded: each of an array `object`, the concise
    /// Relationship that names its family by `object` alone, the previous
    /// targets of `showChanges`, a sub-attribute's, and those of an entity an
    /// inline join nested.
    #[test]
    fn every_target_is_decoded_wherever_its_instance_stands() {
        let mut value = json!({
            "id": "urn:x", "type": "T",
            "many": {"type": "Relationship", "object": ["urn%3Aa", "urn%3Ab"], "previousObject": "urn%3Ac"},
            "concise": {"object": "urn%3Ad", "observedAt": "2020-01-01T00:00:00Z"},
            "speed": {"type": "Property", "value": 1, "measuredBy": {"type": "Relationship", "object": "urn%3Ae"}},
            "list": {"type": "ListRelationship", "objectList": ["urn%3Af"], "previousObjectList": ["urn%3Ag"]},
            "joined": {"type": "Relationship", "object": "urn%3Ah", "entity": {
                "id": "urn:h", "type": "H", "next": {"type": "Relationship", "object": "urn%3Ai"}
            }}
        });
        decode_percent_encoded_uris(&mut value);
        pretty_assertions::assert_eq!(value["many"]["object"], json!(["urn:a", "urn:b"]));
        pretty_assertions::assert_eq!(value["many"]["previousObject"], "urn:c");
        pretty_assertions::assert_eq!(value["concise"]["object"], "urn:d");
        pretty_assertions::assert_eq!(value["speed"]["measuredBy"]["object"], "urn:e");
        pretty_assertions::assert_eq!(value["list"]["objectList"], json!(["urn:f"]));
        pretty_assertions::assert_eq!(value["list"]["previousObjectList"], json!(["urn:g"]));
        pretty_assertions::assert_eq!(value["joined"]["object"], "urn:h");
        pretty_assertions::assert_eq!(value["joined"]["entity"]["next"]["object"], "urn:i");
    }

    #[test]
    fn a_collection_has_every_entity_decoded() {
        let mut value = json!([
            {"id": "urn:a", "type": "T", "ref": {"type": "Relationship", "object": "urn%3Ax"}},
            {"id": "urn:b", "type": "T", "ref": {"type": "Relationship", "object": "urn:y"}}
        ]);
        decode_percent_encoded_uris(&mut value);
        pretty_assertions::assert_eq!(value[0]["ref"]["object"], "urn:x");
        pretty_assertions::assert_eq!(value[1]["ref"]["object"], "urn:y");
    }

    /// The typed walk writes the targets the document rule leaves.
    #[test]
    fn the_typed_targets_are_written_as_the_document_rule_leaves_them() {
        let many = OneOrMany::Many(vec![uri("urn:ngsi-ld:City:x%3A1"), uri("urn:ngsi-ld:City:a%20b")]);
        let decoded = serde_json::to_value(DecodedTargets::new(&many, UriDecoding::Decode)).expect("serialises");
        pretty_assertions::assert_eq!(decoded, json!(["urn:ngsi-ld:City:x:1", "urn:ngsi-ld:City:a%20b"]));
        let kept = serde_json::to_value(DecodedTargets::new(&many, UriDecoding::Keep)).expect("serialises");
        pretty_assertions::assert_eq!(kept, json!(["urn:ngsi-ld:City:x%3A1", "urn:ngsi-ld:City:a%20b"]));
    }
}
