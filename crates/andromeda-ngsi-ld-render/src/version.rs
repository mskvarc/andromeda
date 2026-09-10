//! Payload adaptation for NGSI-LD version conformance (clause 4.3.6.8).
//!
//! Two callers ask for it. A client sending `Prefer: ngsi-ld=<version>` gets a
//! 203 Non-Authoritative Information and a payload whose members and datatypes
//! were removed or reformatted down to the version it named; and a Subscription
//! carrying an `ngsildConformance` gets the same operation applied to the
//! Entities of every notification it is sent (clause 5.8.6).

use crate::object_list::object_list_member_target;
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, spec_version::SpecVersion};
use andromeda_ngsi_ld_query::dataset::DEFAULT_INSTANCE_TOKEN;
use andromeda_ngsi_ld_representation::{entity_members::ENTITY_MEMBERS, response_format::ResponseFormat};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde_json::{Map, Value};
use std::str::FromStr;

/// What a `Prefer: ngsi-ld=<version>` request did to a response payload.
///
/// Clause 6.3.6 makes the `Preference-Applied` echo unconditional once the
/// preference was expressed -- it names the version the body conforms to --
/// while clause 4.3.6.8 makes the status depend on whether the body was
/// amended: a broker that amends a payload down to an older version is no
/// longer speaking authoritatively about it, so that response is 203 rather
/// than 200. Both decisions are taken after rendering by whoever writes the
/// response, so the outcome travels with the rendered output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionAdaptation {
    /// The payload was amended to conform to `version`, an older NGSI-LD
    /// version than the one this broker conforms to.
    Applied {
        /// The version the amended payload conforms to.
        version: SpecVersion,
    },
    /// A version was requested but the payload already conforms to it: the
    /// requested version is the conformant one or newer, so nothing changed.
    Unchanged,
    /// No version preference was expressed.
    None,
}

impl VersionAdaptation {
    /// The version to echo in `Preference-Applied` (clause 6.3.6), when a
    /// preference was expressed at all: the amended version, or the version
    /// this broker conforms to when the payload needed no amendment.
    #[must_use]
    pub const fn applied_version(self) -> Option<SpecVersion> {
        match self {
            VersionAdaptation::Applied { version } => Some(version),
            VersionAdaptation::Unchanged => Some(SpecVersion::CONFORMANT),
            VersionAdaptation::None => None,
        }
    }

    /// Whether the payload was amended, which is what turns a 200 into a 203.
    #[must_use]
    pub const fn is_amended(self) -> bool {
        matches!(self, VersionAdaptation::Applied { .. })
    }
}

/// Adapt a serialized NGSI-LD entity payload to conform to an older spec version.
///
/// Applies the fallback tables from clause 4.3.6.8:
/// - Table 4.3.6.8-1: entity-level removals
/// - Table 4.3.6.8-2: property-level removals
/// - Table 4.3.6.8-3: relationship-level removals
///
/// Handles both single entities (JSON objects) and entity arrays.
///
/// The representation the payload is already rendered in decides how an
/// attribute is read: clause 4.5.1 reifies every attribute into an object
/// naming its `type`, clause 4.5.2.3 drops that `type` and leaves the family
/// to be inferred from the value member, and clause 4.5.4 reduces the whole
/// attribute to its bare value. The default is the normalized reading; set it
/// with [`VersionRenderer::with_format`] when the payload has already been
/// rendered in one of the other two.
#[derive(Debug)]
pub struct VersionRenderer {
    /// The spec version the payload is adapted down to.
    target: SpecVersion,
    /// The representation the payload is already rendered in.
    format: ResponseFormat,
    /// The payload being adapted.
    payload: Value,
}

impl VersionRenderer {
    /// Create a renderer targeting the given NGSI-LD version with an empty payload.
    #[must_use]
    pub const fn new(target: SpecVersion) -> VersionRenderer {
        VersionRenderer {
            target,
            format: ResponseFormat::Normalized,
            payload: Value::Null,
        }
    }

    /// Set the representation the payload is already rendered in.
    #[must_use]
    pub const fn with_format(mut self, format: ResponseFormat) -> VersionRenderer {
        self.format = format;
        self
    }

    /// Set the payload value to adapt.
    #[must_use]
    pub fn with_payload(mut self, payload: Value) -> VersionRenderer {
        self.payload = payload;
        self
    }

    /// Apply the clause 4.3.6.8 fallback tables and return the adapted payload
    /// together with whether adaptation was performed.
    ///
    /// The [`VersionAdaptation`] the caller receives drives the status code: a
    /// requested version that is the current one or newer needs no adaptation,
    /// so the payload is returned unchanged with
    /// [`VersionAdaptation::Unchanged`] and the response stays 200. Only a
    /// target below the conformant version runs the fallback tables, which
    /// makes the representation non-authoritative (203). Removing members
    /// cannot fail, so this answers with a value rather than a `Result` no
    /// caller could act on.
    #[must_use]
    pub fn render(self) -> (Value, VersionAdaptation) {
        let VersionRenderer { target, format, mut payload } = self;
        if target >= SpecVersion::CONFORMANT {
            return (payload, VersionAdaptation::Unchanged);
        }

        match &mut payload {
            Value::Object(_) => adapt_entity(&mut payload, target, format),
            Value::Array(items) => {
                for item in items {
                    adapt_entity(item, target, format);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
        (payload, VersionAdaptation::Applied { version: target })
    }
}

/// The NGSI-LD version each attribute family was introduced at (Table
/// 4.3.6.8-1, "Version Introduced").
///
/// A target older than the version a family was introduced at is a target that
/// never defined the family, so Table 4.3.6.8-1's "Conformant Data Fallback"
/// reformats it as the generalization it specializes: every Property subclass
/// as a Property, and a `ListRelationship` as a Relationship.
const fn family_introduced(family: AttributeType) -> SpecVersion {
    match family {
        AttributeType::Property | AttributeType::GeoProperty | AttributeType::Relationship => SpecVersion::V1_0,
        AttributeType::LanguageProperty => SpecVersion::V1_4,
        AttributeType::VocabProperty | AttributeType::JsonProperty | AttributeType::ListProperty | AttributeType::ListRelationship => SpecVersion::V1_8,
    }
}

/// Adapt a single entity object.
fn adapt_entity(entity: &mut Value, target: SpecVersion, format: ResponseFormat) {
    let Some(obj) = entity.as_object_mut() else {
        return;
    };

    // Table 4.3.6.8-1: entity-level removals
    if target < SpecVersion::V1_9 {
        obj.remove("expiresAt");
    }
    if target < SpecVersion::V1_4 {
        obj.remove("scope");
    }
    // Table 4.3.6.8-1 note 1: a multi-valued `type` exists from 1.3 onwards;
    // an older target receives the first value alone.
    if target < SpecVersion::V1_3
        && let Some(Value::Array(types)) = obj.get_mut("type")
    {
        let first = types.first().cloned().unwrap_or(Value::Null); // clone: the first element outlives the array it is lifted out of
        obj.insert("type".to_string(), first);
    }

    // Collect attribute keys; clause 5.2.4's own members are not attributes and
    // are read from the one table every renderer shares.
    let attr_keys: Vec<String> = obj.keys().filter(|key| !ENTITY_MEMBERS.contains(&key.as_str())).cloned().collect();

    for key in attr_keys {
        if let Some(attr_value) = obj.get_mut(&key) {
            adapt_attribute(attr_value, target, format);
        }
    }
}

/// Adapt one attribute of an entity, in the representation the payload carries.
fn adapt_attribute(value: &mut Value, target: SpecVersion, format: ResponseFormat) {
    match format {
        // Concise is the normalized shape with the inferrable `type` dropped
        // (clause 4.5.2.3), so one walk serves both; which member the family is
        // read from is what differs, and that is decided per instance.
        ResponseFormat::Normalized | ResponseFormat::Concise => adapt_reified_attribute(value, target, format),
        ResponseFormat::KeyValues | ResponseFormat::Simplified => adapt_simplified_attribute(value, target),
    }
}

/// Adapt a reified attribute value (which may be a single instance or an array
/// of instances).
fn adapt_reified_attribute(value: &mut Value, target: SpecVersion, format: ResponseFormat) {
    match value {
        // An array of bare values is the value of one attribute, not several
        // instances of it: clause 4.5.5 separates instances by `datasetId`, and
        // a bare concise or simplified value has nowhere to carry one. Only an
        // array holding an instance object is read as the multi-instance form.
        Value::Array(instances) if instances.iter().any(Value::is_object) => {
            // Notes 2 and 3 of Table 4.3.6.8-1: multi-instance attributes exist
            // from 1.3 onwards; an older target receives the default instance
            // alone, as the bare object a 1.0 attribute is. The default instance
            // is located before the per-instance fallbacks remove `datasetId`.
            if target < SpecVersion::V1_3 {
                // Clause 4.5.5 makes the default instance the one carrying no
                // `datasetId`, which includes an instance that has nowhere to
                // carry one: a concise Property with nothing but a value is the
                // bare value (clause 4.5.2.3), and that is a default instance
                // just as much as an object without the member is.
                let default_idx = instances
                    .iter()
                    .position(|instance| !instance.as_object().is_some_and(|members| members.contains_key("datasetId")))
                    .unwrap_or(0);
                let mut default_instance = if instances.is_empty() {
                    Value::Null
                } else {
                    instances.swap_remove(default_idx)
                };
                adapt_attribute_instance(&mut default_instance, target, format);
                *value = default_instance;
                return;
            }
            for instance in instances.iter_mut() {
                adapt_attribute_instance(instance, target, format);
            }
        }
        Value::Object(_) => {
            adapt_attribute_instance(value, target, format);
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => {}
    }
}

/// The order the family of a reified instance is inferred in when the instance
/// names no `type`.
///
/// Clause 4.5.2.3 makes `type` optional in the concise representation -- "If
/// missing, `Property` can be inferred by the presence of the `value`
/// attribute" -- and each family reserves a distinct value member, so the
/// member that is present names the family. `Property` comes last because it
/// shares `value` with a `GeoProperty`; the two are indistinguishable without
/// resolving the value as a `GeoJSON` geometry, and Table 4.3.6.8-2 gives them
/// identical fallbacks, so the distinction cannot change the outcome.
const INFERENCE_ORDER: [AttributeType; 7] = [
    AttributeType::LanguageProperty,
    AttributeType::VocabProperty,
    AttributeType::JsonProperty,
    AttributeType::ListProperty,
    AttributeType::ListRelationship,
    AttributeType::Relationship,
    AttributeType::Property,
];

/// The attribute family one reified instance belongs to.
///
/// A declared `type` wins; without one the family is inferred from the value
/// member the instance carries. An instance that names neither is not an
/// attribute this rendering understands, and is left alone.
fn reified_family(obj: &Map<String, Value>) -> Option<AttributeType> {
    if let Some(declared) = obj.get("type").and_then(Value::as_str).and_then(|raw| AttributeType::from_str(raw).ok()) {
        return Some(declared);
    }
    INFERENCE_ORDER.into_iter().find(|family| obj.contains_key(family.compact_value_key()))
}

/// Rewrite the `type` of an instance a fallback reformatted -- but only when
/// the representation declares one.
///
/// Clause 4.5.2.3 makes `type` optional in the concise representation, where
/// the family is inferred from the value member instead. Writing one back
/// would reify an attribute the response deliberately left concise, and the
/// value member the reformat just moved already says which family it is now.
fn retype(obj: &mut Map<String, Value>, family: AttributeType) {
    if obj.contains_key("type") {
        obj.insert("type".to_string(), Value::String(family.as_ref().to_owned()));
    }
}

/// Move an instance's value member to the one its generalization carries.
fn move_value_member(obj: &mut Map<String, Value>, from: AttributeType, to: AttributeType) {
    if let Some(value) = obj.remove(from.compact_value_key()) {
        obj.insert(to.compact_value_key().to_string(), value);
    }
}

/// Collapse a `ListRelationship`'s `objectList` down to the single `object` a
/// Relationship carries (Table 4.3.6.8-1).
///
/// Both representations clause 4.5.22 gives the member are read, since the
/// downgrade runs over whichever one the payload was already rendered in.
fn collapse_object_list(obj: &mut Map<String, Value>) {
    if let Some(Value::Array(list)) = obj.remove(compact_properties::OBJECT_LIST) {
        let first = list
            .first()
            .and_then(object_list_member_target)
            .map_or(Value::Null, |target| Value::String(target.to_owned()));
        obj.insert(compact_properties::OBJECT.to_string(), first);
    }
}

/// Adapt a single attribute instance based on its family.
fn adapt_attribute_instance(instance: &mut Value, target: SpecVersion, format: ResponseFormat) {
    let Some(obj) = instance.as_object_mut() else {
        return;
    };
    let Some(family) = reified_family(obj) else {
        return;
    };
    let reformats = target < family_introduced(family);

    match family {
        AttributeType::Property | AttributeType::GeoProperty => adapt_property_fields(obj, target),
        AttributeType::Relationship => adapt_relationship_fields(obj, target),
        AttributeType::LanguageProperty | AttributeType::VocabProperty | AttributeType::JsonProperty | AttributeType::ListProperty => {
            if reformats {
                move_value_member(obj, family, AttributeType::Property);
                retype(obj, AttributeType::Property);
            }
            adapt_property_fields(obj, target);
        }
        AttributeType::ListRelationship => {
            if reformats {
                collapse_object_list(obj);
                retype(obj, AttributeType::Relationship);
            }
            adapt_relationship_fields(obj, target);
        }
    }

    if format == ResponseFormat::Concise {
        collapse_concise_property(instance);
    }
}

/// Whether a bare value reads back as the value that went in (clause 4.5.2.3).
///
/// Two shapes do not survive the collapse. Clause 4.5.5 reads a bare JSON array
/// as the multiple instances of one attribute, and clause 4.5.2.3 says that "if
/// the provided value is a JSON object and contains a `type` field, the whole
/// Attribute shall be treated as a normalized representation". Either would come
/// back as something other than the value it stands for, so those keep the
/// object form -- whose `value` member the same clause makes Mandatory and
/// whose `type` it makes optional.
fn collapses_to_bare_value(value: &Value) -> bool {
    match value {
        Value::Array(_) => false,
        Value::Object(members) => !members.contains_key("type"),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => true,
    }
}

/// Restore the clause 4.5.2.3 collapse to an instance the fallback tables
/// reduced to a bare Property.
///
/// Clause 4.5.2.3: "An NGSI-LD Property without sub-attributes shall be
/// represented in a concise but lossless representation by a member whose key
/// is the Property name and whose value is the Property Value." Removing
/// `observedAt` from a concise Property, or reformatting a `VocabProperty` into
/// one, leaves exactly that shape, so the collapse the concise rendering would
/// have made is made again here. Only a Property collapses: clause 4.5.3.3 and
/// its siblings keep every other family's distinguishing member, and a bare
/// value would read back as a Property.
fn collapse_concise_property(instance: &mut Value) {
    let Some(obj) = instance.as_object_mut() else {
        return;
    };
    let value_key = AttributeType::Property.compact_value_key();
    if obj.len() != 1 || !obj.contains_key(value_key) {
        return;
    }
    let Some(value) = obj.get(value_key) else {
        return;
    };
    if !collapses_to_bare_value(value) {
        return;
    }
    if let Some(value) = obj.remove(value_key) {
        *instance = value;
    }
}

/// The families clause 4.5.4 carries inside a one-member wrapper.
///
/// "In the simplified representation ... a `LanguageProperty` is represented by
/// a JSON object with a single member `languageMap`", and the same holds for a
/// `JsonProperty`'s `json` and a `VocabProperty`'s `vocab`. Every other family
/// is its bare value, which carries nothing Tables 4.3.6.8-2 and -3 name.
const SIMPLIFIED_WRAPPED_FAMILIES: [AttributeType; 3] = [AttributeType::LanguageProperty, AttributeType::VocabProperty, AttributeType::JsonProperty];

/// The sole member of a one-member JSON object, if the value is one.
fn sole_member(value: &Value) -> Option<&str> {
    let obj = value.as_object()?;
    if obj.len() != 1 {
        return None;
    }
    obj.keys().next().map(String::as_str)
}

/// Adapt one attribute of a simplified entity (clause 4.5.4).
///
/// A simplified attribute is its bare value, so none of the members Tables
/// 4.3.6.8-2 and -3 remove is present to remove and only two fallbacks can
/// apply at all: the clause 4.5.4 `dataset` map that carries the multiple
/// instances of Table 4.3.6.8-1 notes 2 and 3, and the one-member wrappers of
/// [`SIMPLIFIED_WRAPPED_FAMILIES`], which the Table 4.3.6.8-1 reformat rows
/// unwrap into the bare Property value a simplified Property is.
///
/// A `ListRelationship` and a `ListProperty` are not reachable here. Clause
/// 4.5.4 renders both as a bare JSON array, which is also what a Property whose
/// value is an array renders as, so the family cannot be recovered from the
/// representation and the Table 4.3.6.8-1 reformat has nothing to key on.
fn adapt_simplified_attribute(value: &mut Value, target: SpecVersion) {
    let Some(member) = sole_member(value) else {
        return;
    };
    if member == compact_properties::DATASET {
        adapt_simplified_dataset_map(value, target);
        return;
    }
    let Some(family) = SIMPLIFIED_WRAPPED_FAMILIES.into_iter().find(|family| family.compact_value_key() == member) else {
        return;
    };
    if target >= family_introduced(family) {
        return;
    }
    if let Some(inner) = value.as_object_mut().and_then(|obj| obj.remove(family.compact_value_key())) {
        *value = inner;
    }
}

/// Adapt the clause 4.5.4 `dataset` map an attribute with several instances is
/// carried in.
///
/// Table 4.3.6.8-1 notes 2 and 3: a target below 1.3 knew no `datasetId`, so it
/// receives one instance, "with preference to the default instance" -- the
/// `@none` key clause 4.5.4 gives it -- and receives it as the bare value a 1.0
/// simplified attribute is, not as a map. A 1.3 or newer target keeps the map,
/// with each instance adapted in its own right.
fn adapt_simplified_dataset_map(value: &mut Value, target: SpecVersion) {
    let Some(datasets) = value.as_object_mut().and_then(|obj| obj.get_mut(compact_properties::DATASET)) else {
        return;
    };
    let Some(datasets) = datasets.as_object_mut() else {
        return;
    };
    if target >= SpecVersion::V1_3 {
        for instance in datasets.values_mut() {
            adapt_simplified_attribute(instance, target);
        }
        return;
    }
    let default_key = if datasets.contains_key(DEFAULT_INSTANCE_TOKEN) {
        Some(DEFAULT_INSTANCE_TOKEN.to_string())
    } else {
        datasets.keys().next().cloned() // clone: the key is read out before the map it indexes is drained
    };
    let Some(default_key) = default_key else {
        return;
    };
    let Some(mut default_instance) = datasets.remove(&default_key) else {
        return;
    };
    adapt_simplified_attribute(&mut default_instance, target);
    *value = default_instance;
}

/// Remove property-level fields per Table 4.3.6.8-2.
fn adapt_property_fields(obj: &mut Map<String, Value>, target: SpecVersion) {
    if target < SpecVersion::V1_3 {
        obj.remove("datasetId");
        obj.remove("observedAt");
        obj.remove("unitCode");
    }
    if target < SpecVersion::V1_9 {
        obj.remove("expiresAt");
        obj.remove("valueType");
    }
}

/// Remove relationship-level fields per Table 4.3.6.8-3.
fn adapt_relationship_fields(obj: &mut Map<String, Value>, target: SpecVersion) {
    if target < SpecVersion::V1_3 {
        obj.remove("datasetId");
        obj.remove("observedAt");
    }
    if target < SpecVersion::V1_8 {
        obj.remove("objectType");
    }
    if target < SpecVersion::V1_9 {
        obj.remove("expiresAt");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn adapt(target: SpecVersion, value: Value) -> Value {
        VersionRenderer::new(target).with_payload(value).render().0
    }

    fn adapt_as(target: SpecVersion, format: ResponseFormat, value: Value) -> Value {
        VersionRenderer::new(target).with_format(format).with_payload(value).render().0
    }

    #[test]
    fn a_conformant_or_newer_target_reports_no_adaptation() {
        // Clause 4.3.6.8: the payload is already authoritative for the current
        // version or newer, so no adaptation is performed and the response is
        // 200, not 203. This is what the short-circuit must report.
        let payload = json!({"id": "urn:x", "type": "T", "expiresAt": "2025-01-01T00:00:00Z"});
        let (_, adaptation) = VersionRenderer::new(SpecVersion::CONFORMANT).with_payload(payload).render();
        pretty_assertions::assert_eq!(adaptation, VersionAdaptation::Unchanged);
        pretty_assertions::assert_eq!(adaptation.applied_version(), Some(SpecVersion::CONFORMANT));
        assert!(!adaptation.is_amended());
    }

    #[test]
    fn a_target_below_the_conformant_version_reports_adaptation_to_that_target() {
        let payload = json!({"id": "urn:x", "type": "T", "expiresAt": "2025-01-01T00:00:00Z"});
        let (_, adaptation) = VersionRenderer::new(SpecVersion::V1_4).with_payload(payload).render();
        pretty_assertions::assert_eq!(adaptation, VersionAdaptation::Applied { version: SpecVersion::V1_4 });
        pretty_assertions::assert_eq!(adaptation.applied_version(), Some(SpecVersion::V1_4));
        assert!(adaptation.is_amended());
    }

    #[test]
    fn an_absent_preference_echoes_no_version() {
        pretty_assertions::assert_eq!(VersionAdaptation::None.applied_version(), None);
        assert!(!VersionAdaptation::None.is_amended());
    }

    #[test]
    fn no_adaptation_for_conformant_version() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "expiresAt": "2025-01-01T00:00:00Z",
            "scope": ["/A"],
            "temperature": {
                "type": "Property",
                "value": 25.0,
                "datasetId": "urn:ds:1",
                "expiresAt": "2025-01-01T00:00:00Z"
            }
        });
        let original = value.clone();
        let adapted = adapt(SpecVersion::V1_9, value);
        pretty_assertions::assert_eq!(adapted, original);
    }

    #[test]
    fn v1_8_removes_entity_expires_at() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "expiresAt": "2025-01-01T00:00:00Z",
            "temperature": {
                "type": "Property",
                "value": 25.0
            }
        });
        let adapted = adapt(SpecVersion::V1_8, value);
        assert!(adapted.get("expiresAt").is_none());
    }

    #[test]
    fn v1_3_removes_scope_from_entity() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "scope": ["/A/B"],
            "temperature": {
                "type": "Property",
                "value": 25.0
            }
        });
        let adapted = adapt(SpecVersion::V1_3, value);
        assert!(adapted.get("scope").is_none());
    }

    #[test]
    fn v1_0_removes_dataset_id_and_observed_at_from_property() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "temperature": {
                "type": "Property",
                "value": 25.0,
                "datasetId": "urn:ds:1",
                "observedAt": "2025-01-01T00:00:00Z",
                "unitCode": "CEL"
            }
        });
        let adapted = adapt(SpecVersion::V1_0, value);
        let temp = adapted.get("temperature").expect("the adapted attribute");
        assert!(temp.get("datasetId").is_none());
        assert!(temp.get("observedAt").is_none());
        assert!(temp.get("unitCode").is_none());
    }

    #[test]
    fn v1_0_removes_dataset_id_and_observed_at_from_relationship() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "refBuilding": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Building:1",
                "datasetId": "urn:ds:1",
                "observedAt": "2025-01-01T00:00:00Z",
                "objectType": "Building"
            }
        });
        let adapted = adapt(SpecVersion::V1_0, value);
        let rel = adapted.get("refBuilding").expect("the adapted attribute");
        assert!(rel.get("datasetId").is_none());
        assert!(rel.get("observedAt").is_none());
        assert!(rel.get("objectType").is_none());
    }

    #[test]
    fn v1_3_reformats_language_property_as_property() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "name": {
                "type": "LanguageProperty",
                "languageMap": {
                    "en": "Tower",
                    "de": "Turm"
                }
            }
        });
        let adapted = adapt(SpecVersion::V1_3, value);
        let name = adapted.get("name").expect("the adapted attribute");
        pretty_assertions::assert_eq!(name.get("type").and_then(|t| t.as_str()), Some("Property"));
        assert!(name.get("languageMap").is_none());
        assert!(name.get("value").is_some());
    }

    #[test]
    fn v1_4_removes_expires_at_and_value_type_from_property() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "temperature": {
                "type": "Property",
                "value": 25.0,
                "expiresAt": "2025-01-01T00:00:00Z",
                "valueType": "Number"
            }
        });
        let adapted = adapt(SpecVersion::V1_4, value);
        let temp = adapted.get("temperature").expect("the adapted attribute");
        assert!(temp.get("expiresAt").is_none());
        assert!(temp.get("valueType").is_none());
    }

    #[test]
    fn v1_4_reformats_vocab_property_as_property() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "category": {
                "type": "VocabProperty",
                "vocab": "commercial"
            }
        });
        let adapted = adapt(SpecVersion::V1_4, value);
        let cat = adapted.get("category").expect("the adapted attribute");
        pretty_assertions::assert_eq!(cat.get("type").and_then(|t| t.as_str()), Some("Property"));
        assert!(cat.get("vocab").is_none());
        pretty_assertions::assert_eq!(cat.get("value").and_then(|v| v.as_str()), Some("commercial"));
    }

    /// Table 5.2.37-1 gives the member two representations, and the downgrade
    /// runs over whichever one the response was rendered in, so the normalized
    /// `object`-keyed member yields the same v1.4 Relationship target as the
    /// concise bare string does.
    #[test]
    fn v1_4_reformats_a_normalized_list_relationship_as_relationship() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:2",
            "type": "TestEntity",
            "refRooms": {
                "type": "ListRelationship",
                "objectList": [{"object": "urn:ngsi-ld:Room:1"}, {"object": "urn:ngsi-ld:Room:2"}]
            }
        });
        let adapted = adapt(SpecVersion::V1_4, value);
        let rel = adapted.get("refRooms").expect("the downgraded attribute");
        pretty_assertions::assert_eq!(rel.get("type").and_then(|t| t.as_str()), Some("Relationship"));
        assert!(rel.get("objectList").is_none());
        pretty_assertions::assert_eq!(rel.get("object").and_then(|v| v.as_str()), Some("urn:ngsi-ld:Room:1"));
    }

    #[test]
    fn v1_4_reformats_list_relationship_as_relationship() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "refRooms": {
                "type": "ListRelationship",
                "objectList": ["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"]
            }
        });
        let adapted = adapt(SpecVersion::V1_4, value);
        let rel = adapted.get("refRooms").expect("the adapted attribute");
        pretty_assertions::assert_eq!(rel.get("type").and_then(|t| t.as_str()), Some("Relationship"));
        assert!(rel.get("objectList").is_none());
        pretty_assertions::assert_eq!(rel.get("object").and_then(|v| v.as_str()), Some("urn:ngsi-ld:Room:1"));
    }

    #[test]
    fn v1_0_multi_attribute_returns_default_instance() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "temperature": [
                {
                    "type": "Property",
                    "value": 25.0,
                    "datasetId": "urn:ds:1"
                },
                {
                    "type": "Property",
                    "value": 30.0
                }
            ]
        });
        let adapted = adapt(SpecVersion::V1_0, value);
        let temp = adapted.get("temperature").expect("the adapted attribute");
        assert!(temp.is_object(), "a 1.0 attribute is a bare instance object, not an array: {temp}");
        pretty_assertions::assert_eq!(temp.get("value").and_then(serde_json::Value::as_f64), Some(30.0));
        assert!(temp.get("datasetId").is_none());
    }

    /// Table 4.3.6.8-1 note 1: a v1.0 target receives a single `type`, the
    /// first of the values the entity declared.
    #[test]
    fn v1_0_multi_valued_type_collapses_to_its_first_value() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": ["Vehicle", "Car"],
            "speed": {"type": "Property", "value": 10}
        });
        let adapted = adapt(SpecVersion::V1_0, value);
        pretty_assertions::assert_eq!(adapted.get("type"), Some(&json!("Vehicle")));

        let kept = adapt(SpecVersion::V1_3, json!({"id": "urn:ngsi-ld:Entity:1", "type": ["Vehicle", "Car"]}));
        pretty_assertions::assert_eq!(kept.get("type"), Some(&json!(["Vehicle", "Car"])));
    }

    /// Notes 2 and 3: a multi-instance attribute whose instances all carry a
    /// `datasetId` has no default instance, so the first one stands in, and a
    /// 1.3 target keeps the array untouched.
    #[test]
    fn v1_0_multi_instance_without_a_default_collapses_to_the_first_instance_object() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 10, "datasetId": "urn:ngsi-ld:ds:1"},
                {"type": "Property", "value": 20, "datasetId": "urn:ngsi-ld:ds:2"}
            ]
        });
        let adapted = adapt(SpecVersion::V1_0, value.clone());
        pretty_assertions::assert_eq!(adapted["speed"], json!({"type": "Property", "value": 10}));

        let kept = adapt(SpecVersion::V1_3, value);
        pretty_assertions::assert_eq!(kept["speed"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn adapts_entity_array() {
        let value = json!([
            {
                "id": "urn:ngsi-ld:Entity:1",
                "type": "TestEntity",
                "expiresAt": "2025-01-01T00:00:00Z"
            },
            {
                "id": "urn:ngsi-ld:Entity:2",
                "type": "TestEntity",
                "scope": ["/A"]
            }
        ]);
        let adapted = adapt(SpecVersion::V1_3, value);
        let arr = adapted.as_array().expect("an adapted array");
        assert!(arr[0].get("expiresAt").is_none());
        assert!(arr[1].get("scope").is_none());
    }

    #[test]
    fn v1_4_reformats_json_property_as_property() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "config": {
                "type": "JsonProperty",
                "json": {"key": "value"}
            }
        });
        let adapted = adapt(SpecVersion::V1_4, value);
        let config = adapted.get("config").expect("the adapted attribute");
        pretty_assertions::assert_eq!(config.get("type").and_then(|t| t.as_str()), Some("Property"));
        assert!(config.get("json").is_none());
        assert!(config.get("value").is_some());
    }

    #[test]
    fn v1_4_reformats_list_property_as_property() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "tags": {
                "type": "ListProperty",
                "valueList": ["a", "b", "c"]
            }
        });
        let adapted = adapt(SpecVersion::V1_4, value);
        let tags = adapted.get("tags").expect("the adapted attribute");
        pretty_assertions::assert_eq!(tags.get("type").and_then(|t| t.as_str()), Some("Property"));
        assert!(tags.get("valueList").is_none());
        pretty_assertions::assert_eq!(tags.get("value").expect("the moved value member"), &json!(["a", "b", "c"]));
    }

    #[test]
    fn v1_4_removes_object_type_from_relationship() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "refBuilding": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Building:1",
                "objectType": "Building"
            }
        });
        let adapted = adapt(SpecVersion::V1_4, value);
        let rel = adapted.get("refBuilding").expect("the adapted attribute");
        assert!(rel.get("objectType").is_none());
    }

    #[test]
    fn v1_4_removes_expires_at_from_relationship() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "refBuilding": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Building:1",
                "expiresAt": "2025-01-01T00:00:00Z"
            }
        });
        let adapted = adapt(SpecVersion::V1_4, value);
        let rel = adapted.get("refBuilding").expect("the adapted attribute");
        assert!(rel.get("expiresAt").is_none());
    }

    /// Clause 4.5.2.3 makes `type` optional in the concise representation, so
    /// the family is inferred from the value member. A concise
    /// `LanguageProperty` therefore reformats to a Property for a 1.3 target
    /// exactly as its normalized twin does, and collapses to the bare Property
    /// value the same clause mandates for a Property without sub-attributes.
    #[test]
    fn a_concise_language_property_reformats_and_collapses_for_a_1_3_target() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "name": {"languageMap": {"en": "Tower", "de": "Turm"}}
        });
        let adapted = adapt_as(SpecVersion::V1_3, ResponseFormat::Concise, value);
        pretty_assertions::assert_eq!(adapted["name"], json!({"en": "Tower", "de": "Turm"}));
    }

    /// Table 4.3.6.8-2: `observedAt`, `unitCode`, `datasetId` and `valueType`
    /// go from a concise Property just as they go from a normalized one, and
    /// what is left is the clause 4.5.2.3 bare value.
    #[test]
    fn a_concise_property_loses_its_metadata_and_collapses_for_a_1_0_target() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "temperature": {"value": 25.0, "observedAt": "2025-01-01T00:00:00Z", "unitCode": "CEL", "valueType": "Number"}
        });
        let adapted = adapt_as(SpecVersion::V1_0, ResponseFormat::Concise, value);
        pretty_assertions::assert_eq!(adapted["temperature"], json!(25.0));
    }

    /// Clause 4.5.5 reads a bare JSON array as several instances of one
    /// attribute, and clause 4.5.2.3 reads a bare object carrying a `type`
    /// member as a normalized attribute, so neither collapses: both keep the
    /// object form whose `value` member clause 4.5.2.3 makes Mandatory.
    #[test]
    fn a_concise_value_that_would_be_misread_bare_keeps_its_object_form() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "tags": {"valueList": ["a", "b"]},
            "config": {"json": {"type": "config", "level": 3}}
        });
        let adapted = adapt_as(SpecVersion::V1_4, ResponseFormat::Concise, value);
        pretty_assertions::assert_eq!(adapted["tags"], json!({"value": ["a", "b"]}));
        pretty_assertions::assert_eq!(adapted["config"], json!({"value": {"type": "config", "level": 3}}));
    }

    /// A concise Relationship keeps its `object` member (clause 4.5.3.3) but
    /// still loses the members Table 4.3.6.8-3 removes.
    #[test]
    fn a_concise_relationship_loses_its_object_type_and_keeps_its_object() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "refBuilding": {"object": "urn:ngsi-ld:Building:1", "objectType": "Building", "observedAt": "2025-01-01T00:00:00Z"}
        });
        let adapted = adapt_as(SpecVersion::V1_0, ResponseFormat::Concise, value);
        pretty_assertions::assert_eq!(adapted["refBuilding"], json!({"object": "urn:ngsi-ld:Building:1"}));
    }

    /// A concise multi-instance attribute is an array of instances (clause
    /// 4.5.5), which Table 4.3.6.8-1 note 2 reduces to the default instance for
    /// a target below 1.3. The concise rendering collapses the default instance
    /// to its bare value (clause 4.5.2.3), so the instance that carries no
    /// `datasetId` is the one that is not an object at all.
    #[test]
    fn a_concise_multi_instance_attribute_collapses_to_its_default_instance() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "temperature": [{"value": 25.0, "datasetId": "urn:ds:1"}, 30.0]
        });
        let adapted = adapt_as(SpecVersion::V1_0, ResponseFormat::Concise, value);
        pretty_assertions::assert_eq!(adapted["temperature"], json!(30.0));

        let objects = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "temperature": [{"value": 25.0, "datasetId": "urn:ds:1"}, {"value": 30.0, "observedAt": "2025-01-01T00:00:00Z"}]
        });
        let adapted = adapt_as(SpecVersion::V1_0, ResponseFormat::Concise, objects);
        pretty_assertions::assert_eq!(adapted["temperature"], json!(30.0));
    }

    /// Clause 4.5.4 wraps a `LanguageProperty`, a `JsonProperty` and a
    /// `VocabProperty` in a one-member object; Table 4.3.6.8-1 reformats each
    /// as a Property, whose simplified rendering is the bare value.
    #[test]
    fn a_simplified_wrapped_family_unwraps_to_its_bare_value() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "name": {"languageMap": {"en": "Tower"}},
            "category": {"vocab": "commercial"},
            "config": {"json": {"key": "value"}}
        });
        let adapted = adapt_as(SpecVersion::V1_3, ResponseFormat::KeyValues, value);
        pretty_assertions::assert_eq!(adapted["name"], json!({"en": "Tower"}));
        // 1.3 predates neither the JsonProperty nor the VocabProperty reformat
        // threshold of 1.8, so both unwrap as well.
        pretty_assertions::assert_eq!(adapted["category"], json!("commercial"));
        pretty_assertions::assert_eq!(adapted["config"], json!({"key": "value"}));
    }

    /// A target at or above the version a family was introduced at keeps the
    /// clause 4.5.4 wrapper: 1.4 defined the `LanguageProperty`, and 1.8 the
    /// `VocabProperty`.
    #[test]
    fn a_simplified_wrapper_survives_a_target_that_already_knew_the_family() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "name": {"languageMap": {"en": "Tower"}},
            "category": {"vocab": "commercial"}
        });
        let adapted = adapt_as(SpecVersion::V1_4, ResponseFormat::Simplified, value);
        pretty_assertions::assert_eq!(adapted["name"], json!({"languageMap": {"en": "Tower"}}));
        pretty_assertions::assert_eq!(adapted["category"], json!("commercial"));

        let newer = adapt_as(
            SpecVersion::V1_8,
            ResponseFormat::Simplified,
            json!({"id": "urn:ngsi-ld:Entity:1", "type": "TestEntity", "category": {"vocab": "commercial"}}),
        );
        pretty_assertions::assert_eq!(newer["category"], json!({"vocab": "commercial"}));
    }

    /// Clause 4.5.4 carries several instances in a `dataset` map keyed by
    /// `datasetId`, with "the default datasetId (where present) ... represented
    /// by the JSON-LD keyword `@none`". Table 4.3.6.8-1 note 2 reduces that to
    /// the default instance alone for a target below 1.3.
    #[test]
    fn a_simplified_dataset_map_collapses_to_its_default_instance() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "temperature": {"dataset": {"urn:ds:1": 25.0, "@none": 30.0}}
        });
        let adapted = adapt_as(SpecVersion::V1_0, ResponseFormat::KeyValues, value);
        pretty_assertions::assert_eq!(adapted["temperature"], json!(30.0));

        let kept = adapt_as(
            SpecVersion::V1_3,
            ResponseFormat::KeyValues,
            json!({"id": "urn:ngsi-ld:Entity:1", "type": "TestEntity", "temperature": {"dataset": {"urn:ds:1": 25.0, "@none": 30.0}}}),
        );
        pretty_assertions::assert_eq!(kept["temperature"], json!({"dataset": {"urn:ds:1": 25.0, "@none": 30.0}}));
    }

    /// A `dataset` map with no `@none` entry has no default instance, so the
    /// first one stands in, and the wrapper it carries is unwrapped in turn.
    #[test]
    fn a_simplified_dataset_map_without_a_default_takes_its_first_instance() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "name": {"dataset": {"urn:ds:1": {"languageMap": {"en": "Tower"}}}}
        });
        let adapted = adapt_as(SpecVersion::V1_0, ResponseFormat::Simplified, value);
        pretty_assertions::assert_eq!(adapted["name"], json!({"en": "Tower"}));
    }

    /// The entity's own members are reduced in every representation: Table
    /// 4.3.6.8-1 removes `expiresAt` below 1.9 and `scope` below 1.4, and note
    /// 1 collapses a multi-valued `type` below 1.3.
    #[test]
    fn the_entity_members_are_reduced_in_the_simplified_representation_too() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": ["Vehicle", "Car"],
            "scope": ["/A"],
            "expiresAt": "2025-01-01T00:00:00Z",
            "speed": 10
        });
        let adapted = adapt_as(SpecVersion::V1_0, ResponseFormat::KeyValues, value);
        pretty_assertions::assert_eq!(adapted["type"], json!("Vehicle"));
        assert!(adapted.get("scope").is_none(), "{adapted}");
        assert!(adapted.get("expiresAt").is_none(), "{adapted}");
        pretty_assertions::assert_eq!(adapted["speed"], json!(10));
    }

    /// Table 4.3.6.8-1 names each family's introduction, and the reformat rows
    /// apply below it and not at or above it.
    #[test]
    fn each_family_is_reformatted_only_below_the_version_that_introduced_it() {
        pretty_assertions::assert_eq!(family_introduced(AttributeType::Property), SpecVersion::V1_0);
        pretty_assertions::assert_eq!(family_introduced(AttributeType::GeoProperty), SpecVersion::V1_0);
        pretty_assertions::assert_eq!(family_introduced(AttributeType::Relationship), SpecVersion::V1_0);
        pretty_assertions::assert_eq!(family_introduced(AttributeType::LanguageProperty), SpecVersion::V1_4);
        pretty_assertions::assert_eq!(family_introduced(AttributeType::VocabProperty), SpecVersion::V1_8);
        pretty_assertions::assert_eq!(family_introduced(AttributeType::JsonProperty), SpecVersion::V1_8);
        pretty_assertions::assert_eq!(family_introduced(AttributeType::ListProperty), SpecVersion::V1_8);
        pretty_assertions::assert_eq!(family_introduced(AttributeType::ListRelationship), SpecVersion::V1_8);

        let language = json!({"id": "urn:x", "type": "T", "name": {"type": "LanguageProperty", "languageMap": {"en": "Tower"}}});
        let kept = adapt(SpecVersion::V1_4, language);
        pretty_assertions::assert_eq!(kept["name"]["type"], json!("LanguageProperty"));
    }

    /// Clause 4.5.5 separates the instances of one attribute by `datasetId`,
    /// which a bare value has nowhere to carry, so a concise Property whose
    /// value is an array of bare values keeps every element rather than being
    /// reduced to the first as a multi-instance attribute would be.
    #[test]
    fn a_concise_array_of_bare_values_is_one_attribute_value_and_not_several_instances() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "tags": ["a", "b", "c"]
        });
        let adapted = adapt_as(SpecVersion::V1_0, ResponseFormat::Concise, value);
        pretty_assertions::assert_eq!(adapted["tags"], json!(["a", "b", "c"]));
    }

    /// A `GeoProperty` value is a `GeoJSON` geometry whose `type` names a
    /// geometry rather than an attribute family, so the concise inference must
    /// not read it as an attribute and must leave it exactly as it is.
    #[test]
    fn a_concise_geo_property_value_is_not_read_as_an_attribute() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "location": {"type": "Point", "coordinates": [1.0, 2.0]}
        });
        let adapted = adapt_as(SpecVersion::V1_0, ResponseFormat::Concise, value);
        pretty_assertions::assert_eq!(adapted["location"], json!({"type": "Point", "coordinates": [1.0, 2.0]}));
    }
}
