//! The clause 4.3.6.8 fallbacks over an attribute that is still reified.
//!
//! Clause 4.5.1 reifies every attribute into an object naming its `type`, and
//! clause 4.5.2.3 drops that `type` and leaves the family to be inferred from
//! the value member. Both therefore present an attribute as an object whose
//! members Tables 4.3.6.8-2 and -3 name one by one, which is what this module
//! walks. The lossy rendering of clause 4.5.4 keeps none of those members and
//! is adapted by [`crate::version::simplified_attribute`] instead.

use crate::{
    concise::{InstanceStanding, collapses_to_bare_value, keeps_family_type},
    object_list::object_list_member_target,
    version::{adapted_payload::AdaptedPayload, family_introduction::family_introduced},
};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, spec_version::SpecVersion};
use andromeda_ngsi_ld_representation::{
    concise_reading::{ArrayReading, ObjectReading, read_array, read_object},
    response_format::ResponseFormat,
};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde_json::{Map, Value};

/// Adapt a reified attribute value, which may be a single instance or an array
/// of instances.
///
/// What that array is depends on which Element the payload is: in an Entity it
/// is the clause 4.5.5 set of instances separated by `datasetId`, and in the
/// Temporal Evolution of one it is the clause 4.5.7 sequence of instances in
/// time. [`AdaptedPayload`] carries that reading.
pub(crate) fn adapt_reified_attribute(value: &mut Value, target: SpecVersion, format: ResponseFormat, adapted_payload: AdaptedPayload) {
    match value {
        // Only an array the clause 4.5.2.3 reading takes for instances is the
        // multi-instance form: an array of values is the value of one
        // attribute, written bare.
        Value::Array(instances) if read_array(instances) == Ok(ArrayReading::Instances) => {
            // Notes 2 and 3 of Table 4.3.6.8-1: multi-instance attributes exist
            // from 1.3 onwards; an older target receives the default instance
            // alone, as the bare object a 1.0 attribute is. The default instance
            // is located before the per-instance fallbacks remove `datasetId`.
            // Both notes speak of instances "separated by datasetId", which a
            // clause 4.5.7 temporal sequence is not, so the collapse does not
            // reach one.
            if target < SpecVersion::V1_3 && adapted_payload.collapses_instances() {
                // Clause 4.5.5 makes the default instance the one carrying no
                // `datasetId`.
                let default_idx = instances
                    .iter()
                    .position(|instance| !instance.as_object().is_some_and(|members| members.contains_key("datasetId")))
                    .unwrap_or(0);
                let mut default_instance = if instances.is_empty() {
                    Value::Null
                } else {
                    instances.swap_remove(default_idx)
                };
                adapt_attribute_instance(&mut default_instance, target, format, adapted_payload, InstanceStanding::Alone);
                *value = default_instance;
                return;
            }
            for instance in instances.iter_mut() {
                adapt_attribute_instance(instance, target, format, adapted_payload, InstanceStanding::AmongInstances);
            }
        }
        Value::Object(_) => {
            adapt_attribute_instance(value, target, format, adapted_payload, InstanceStanding::Alone);
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => {}
    }
}

/// The attribute family one reified instance belongs to.
///
/// A declared `type` wins; without one the family is the one the clause
/// 4.5.2.3 reading infers from the value member the instance carries, which
/// tells a `GeoProperty` from a Property by its geometry. An instance that
/// names neither is not an attribute this rendering understands, and is left
/// alone.
fn reified_family(obj: &Map<String, Value>) -> Option<AttributeType> {
    match read_object(obj) {
        Ok(ObjectReading::Instance(family)) => Some(family),
        Ok(ObjectReading::GeoPropertyValue | ObjectReading::PropertyValue) | Err(_) => None,
    }
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
fn adapt_attribute_instance(instance: &mut Value, target: SpecVersion, format: ResponseFormat, adapted_payload: AdaptedPayload, standing: InstanceStanding) {
    let Some(obj) = instance.as_object_mut() else {
        return;
    };
    let Some(family) = reified_family(obj) else {
        return;
    };
    let reformats = target < family_introduced(family);

    let adapted_family = match family {
        AttributeType::Property | AttributeType::GeoProperty => {
            adapt_property_fields(obj, target, adapted_payload);
            family
        }
        AttributeType::Relationship => {
            adapt_relationship_fields(obj, target, adapted_payload);
            family
        }
        AttributeType::LanguageProperty | AttributeType::VocabProperty | AttributeType::JsonProperty | AttributeType::ListProperty => {
            let adapted_family = if reformats {
                move_value_member(obj, family, AttributeType::Property);
                retype(obj, AttributeType::Property);
                AttributeType::Property
            } else {
                family
            };
            adapt_property_fields(obj, target, adapted_payload);
            adapted_family
        }
        AttributeType::ListRelationship => {
            let adapted_family = if reformats {
                collapse_object_list(obj);
                retype(obj, AttributeType::Relationship);
                AttributeType::Relationship
            } else {
                family
            };
            adapt_relationship_fields(obj, target, adapted_payload);
            adapted_family
        }
    };

    if format == ResponseFormat::Concise {
        conclude_concise_instance(instance, adapted_family, standing);
    }
}

/// Restore the clause 4.5.2.3 concise shape to an instance the fallback tables
/// changed.
///
/// Clause 4.5.2.3: "An NGSI-LD Property without sub-attributes shall be
/// represented in a concise but lossless representation by a member whose key
/// is the Property name and whose value is the Property Value." Removing
/// `observedAt` from a concise Property, or reformatting a `VocabProperty` into
/// one, can leave a Property or `GeoProperty` carrying nothing but its value,
/// so the collapse the concise rendering would have made is made again here,
/// on the same terms ([`collapses_to_bare_value`]). Where it is not made, a
/// reformatted value that would leave the family misread names it in `type`
/// ([`keeps_family_type`]). Clause 4.5.3.3 and its siblings keep every other
/// family's value member, which names the family.
fn conclude_concise_instance(instance: &mut Value, family: AttributeType, standing: InstanceStanding) {
    if !matches!(family, AttributeType::Property | AttributeType::GeoProperty) {
        return;
    }
    let Some(obj) = instance.as_object_mut() else {
        return;
    };
    let value_key = family.compact_value_key();
    let Some(value) = obj.get(value_key) else {
        return;
    };
    let carries_only_its_value = obj.keys().all(|member| member == value_key || member == "type");
    if carries_only_its_value && collapses_to_bare_value(family, value, standing) {
        if let Some(value) = obj.remove(value_key) {
            *instance = value;
        }
        return;
    }
    if keeps_family_type(family, value) && !obj.contains_key("type") {
        obj.insert("type".to_string(), Value::String(family.as_ref().to_owned()));
    }
}

/// Remove property-level fields per Table 4.3.6.8-2.
///
/// `observedAt` is the one row that does not reach a temporal instance: clause
/// 4.5.7 makes it the Temporal Property recording the point in time the
/// instance stands at, so removing it would leave a payload that is no version
/// of a temporal representation rather than an older one.
fn adapt_property_fields(obj: &mut Map<String, Value>, target: SpecVersion, adapted_payload: AdaptedPayload) {
    if target < SpecVersion::V1_3 {
        obj.remove("datasetId");
        obj.remove("unitCode");
        if adapted_payload.removes_observed_at() {
            obj.remove("observedAt");
        }
    }
    if target < SpecVersion::V1_9 {
        obj.remove("expiresAt");
        obj.remove("valueType");
    }
}

/// Remove relationship-level fields per Table 4.3.6.8-3.
///
/// `observedAt` is held back for a temporal instance for the reason clause
/// 4.5.8 gives it there, which is the one clause 4.5.7 gives a Property's.
fn adapt_relationship_fields(obj: &mut Map<String, Value>, target: SpecVersion, adapted_payload: AdaptedPayload) {
    if target < SpecVersion::V1_3 {
        obj.remove("datasetId");
        if adapted_payload.removes_observed_at() {
            obj.remove("observedAt");
        }
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
    use crate::version::{adapted_payload::AdaptedPayload, reified_attribute::adapt_reified_attribute};
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
    use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
    use serde_json::{Value, json};

    /// Adapt one attribute of an Entity, in the representation the payload was
    /// rendered in.
    fn adapt(target: SpecVersion, format: ResponseFormat, mut attribute: Value) -> Value {
        adapt_reified_attribute(&mut attribute, target, format, AdaptedPayload::Entity);
        attribute
    }

    /// Adapt one attribute of a Temporal Evolution, in the normalized reading
    /// clause 4.5.6 gives it.
    fn temporal(target: SpecVersion, mut attribute: Value) -> Value {
        adapt_reified_attribute(&mut attribute, target, ResponseFormat::Normalized, AdaptedPayload::TemporalEvolution);
        attribute
    }

    /// The normalized rendering of clause 4.5.1, which every entity read
    /// defaults to.
    fn normalized(target: SpecVersion, attribute: Value) -> Value {
        adapt(target, ResponseFormat::Normalized, attribute)
    }

    /// The concise rendering of clause 4.5.2.3, whose `type` is inferred.
    fn concise(target: SpecVersion, attribute: Value) -> Value {
        adapt(target, ResponseFormat::Concise, attribute)
    }

    /// Clause 4.5.7 makes a temporal attribute's array the sequence of its
    /// instances in time, which is not the `datasetId`-separated set notes 2 and
    /// 3 of Table 4.3.6.8-1 collapse, so a 1.0 target still receives the whole
    /// sequence.
    #[test]
    fn a_temporal_sequence_survives_a_target_that_collapses_an_entity_attribute() {
        let history = json!([
            {"type": "Property", "value": 1, "observedAt": "2026-01-01T00:00:00Z"},
            {"type": "Property", "value": 2, "observedAt": "2026-01-02T00:00:00Z"},
            {"type": "Property", "value": 3, "observedAt": "2026-01-03T00:00:00Z"}
        ]);

        let adapted = temporal(SpecVersion::V1_0, history.clone());
        let instances = adapted.as_array().expect("the sequence stays an array: {adapted}");
        pretty_assertions::assert_eq!(instances.len(), 3);
        pretty_assertions::assert_eq!(adapted, history);

        // The same array in an Entity is the multi-instance form, and there the
        // notes do collapse it.
        let collapsed = normalized(SpecVersion::V1_0, history);
        assert!(collapsed.is_object(), "an Entity attribute collapses: {collapsed}");
    }

    /// Clause 4.5.7: `observedAt` is the Temporal Property recording the point
    /// in time an instance stands at, so the Table 4.3.6.8-2 row that removes it
    /// below 1.3 does not reach a temporal instance. The rest of the row does.
    #[test]
    fn a_temporal_property_instance_keeps_its_temporal_index_and_loses_the_rest() {
        let adapted = temporal(
            SpecVersion::V1_0,
            json!([{
                "type": "Property",
                "value": 25.0,
                "observedAt": "2026-01-01T00:00:00Z",
                "datasetId": "urn:ngsi-ld:Dataset:1",
                "unitCode": "CEL"
            }]),
        );
        let instance = &adapted.as_array().expect("a sequence")[0];
        pretty_assertions::assert_eq!(instance.get("observedAt"), Some(&json!("2026-01-01T00:00:00Z")));
        assert!(instance.get("datasetId").is_none(), "{instance}");
        assert!(instance.get("unitCode").is_none(), "{instance}");
    }

    /// Clause 4.5.8 gives a Relationship's temporal instance the same Temporal
    /// Property, so Table 4.3.6.8-3's `observedAt` row is held back the same way.
    #[test]
    fn a_temporal_relationship_instance_keeps_its_temporal_index() {
        let adapted = temporal(
            SpecVersion::V1_0,
            json!([{
                "type": "Relationship",
                "object": "urn:ngsi-ld:Vehicle:1",
                "observedAt": "2026-01-01T00:00:00Z",
                "datasetId": "urn:ngsi-ld:Dataset:1"
            }]),
        );
        let instance = &adapted.as_array().expect("a sequence")[0];
        pretty_assertions::assert_eq!(instance.get("observedAt"), Some(&json!("2026-01-01T00:00:00Z")));
        assert!(instance.get("datasetId").is_none(), "{instance}");
    }

    /// Every fallback that is not one of those two rows applies to a temporal
    /// instance exactly as it does to an Entity's: a `LanguageProperty` arrived
    /// at 1.4 and is reformatted as a Property for an older target.
    #[test]
    fn a_temporal_instance_still_takes_the_family_reformats() {
        let adapted = temporal(
            SpecVersion::V1_3,
            json!([{"type": "LanguageProperty", "languageMap": {"en": "Stop"}, "observedAt": "2026-01-01T00:00:00Z"}]),
        );
        let instance = &adapted.as_array().expect("a sequence")[0];
        pretty_assertions::assert_eq!(instance.get("type"), Some(&json!("Property")));
        pretty_assertions::assert_eq!(instance.get("value"), Some(&json!({"en": "Stop"})));
        pretty_assertions::assert_eq!(instance.get("observedAt"), Some(&json!("2026-01-01T00:00:00Z")));
    }

    /// Table 4.3.6.8-2: `datasetId`, `observedAt` and `unitCode` arrived at 1.3,
    /// so a 1.0 target receives none of them.
    #[test]
    fn v1_0_removes_dataset_id_observed_at_and_unit_code_from_a_property() {
        let adapted = normalized(
            SpecVersion::V1_0,
            json!({"type": "Property", "value": 25.0, "datasetId": "urn:ds:1", "observedAt": "2025-01-01T00:00:00Z", "unitCode": "CEL"}),
        );
        pretty_assertions::assert_eq!(adapted, json!({"type": "Property", "value": 25.0}));
    }

    /// Table 4.3.6.8-2: `expiresAt` and `valueType` arrived at 1.9, so a 1.4
    /// target receives neither while everything 1.4 knew survives.
    #[test]
    fn v1_4_removes_expires_at_and_value_type_from_a_property() {
        let adapted = normalized(
            SpecVersion::V1_4,
            json!({"type": "Property", "value": 25.0, "observedAt": "2025-01-01T00:00:00Z", "expiresAt": "2025-01-01T00:00:00Z", "valueType": "Number"}),
        );
        pretty_assertions::assert_eq!(adapted, json!({"type": "Property", "value": 25.0, "observedAt": "2025-01-01T00:00:00Z"}));
    }

    /// Table 4.3.6.8-3: `datasetId` and `observedAt` arrived at 1.3 and
    /// `objectType` at 1.8, so a 1.0 target receives the bare Relationship.
    #[test]
    fn v1_0_removes_dataset_id_observed_at_and_object_type_from_a_relationship() {
        let adapted = normalized(
            SpecVersion::V1_0,
            json!({"type": "Relationship", "object": "urn:ngsi-ld:Building:1", "datasetId": "urn:ds:1", "observedAt": "2025-01-01T00:00:00Z", "objectType": "Building"}),
        );
        pretty_assertions::assert_eq!(adapted, json!({"type": "Relationship", "object": "urn:ngsi-ld:Building:1"}));
    }

    /// Table 4.3.6.8-3: `expiresAt` arrived at 1.9 and `objectType` at 1.8, so a
    /// 1.4 target receives neither.
    #[test]
    fn v1_4_removes_expires_at_and_object_type_from_a_relationship() {
        let adapted = normalized(
            SpecVersion::V1_4,
            json!({"type": "Relationship", "object": "urn:ngsi-ld:Building:1", "objectType": "Building", "expiresAt": "2025-01-01T00:00:00Z"}),
        );
        pretty_assertions::assert_eq!(adapted, json!({"type": "Relationship", "object": "urn:ngsi-ld:Building:1"}));
    }

    /// Table 4.3.6.8-1: the `LanguageProperty` arrived at 1.4, so a 1.3 target
    /// receives it "Reformat[ted] ... as Property" -- the `languageMap` under
    /// the `value` member a Property carries.
    #[test]
    fn v1_3_reformats_a_language_property_as_a_property() {
        let adapted = normalized(
            SpecVersion::V1_3,
            json!({"type": "LanguageProperty", "languageMap": {"en": "Tower", "de": "Turm"}}),
        );
        pretty_assertions::assert_eq!(adapted, json!({"type": "Property", "value": {"en": "Tower", "de": "Turm"}}));
    }

    /// Table 4.3.6.8-1: the `VocabProperty`, `JsonProperty` and `ListProperty`
    /// all arrived at 1.8, so a 1.4 target receives each as a Property.
    #[test]
    fn v1_4_reformats_the_1_8_property_subclasses_as_properties() {
        pretty_assertions::assert_eq!(
            normalized(SpecVersion::V1_4, json!({"type": "VocabProperty", "vocab": "commercial"})),
            json!({"type": "Property", "value": "commercial"})
        );
        pretty_assertions::assert_eq!(
            normalized(SpecVersion::V1_4, json!({"type": "JsonProperty", "json": {"key": "value"}})),
            json!({"type": "Property", "value": {"key": "value"}})
        );
        pretty_assertions::assert_eq!(
            normalized(SpecVersion::V1_4, json!({"type": "ListProperty", "valueList": ["a", "b", "c"]})),
            json!({"type": "Property", "value": ["a", "b", "c"]})
        );
    }

    /// Table 4.3.6.8-1: the `ListRelationship` arrived at 1.8. Table 5.2.37-1
    /// gives `objectList` two representations, and the downgrade runs over
    /// whichever one the response was rendered in, so both yield the same 1.4
    /// Relationship target.
    #[test]
    fn v1_4_reformats_a_list_relationship_as_a_relationship_in_either_object_list_shape() {
        pretty_assertions::assert_eq!(
            normalized(
                SpecVersion::V1_4,
                json!({"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Room:1"}, {"object": "urn:ngsi-ld:Room:2"}]})
            ),
            json!({"type": "Relationship", "object": "urn:ngsi-ld:Room:1"})
        );
        pretty_assertions::assert_eq!(
            normalized(
                SpecVersion::V1_4,
                json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"]})
            ),
            json!({"type": "Relationship", "object": "urn:ngsi-ld:Room:1"})
        );
    }

    /// A target at or above the version a family was introduced at keeps the
    /// family it was sent: 1.4 defined the `LanguageProperty`.
    #[test]
    fn a_family_survives_a_target_that_already_knew_it() {
        let adapted = normalized(SpecVersion::V1_4, json!({"type": "LanguageProperty", "languageMap": {"en": "Tower"}}));
        pretty_assertions::assert_eq!(adapted, json!({"type": "LanguageProperty", "languageMap": {"en": "Tower"}}));
    }

    /// Table 4.3.6.8-1 note 2: a 1.0 target receives "a single element of
    /// Property Array with preference to the default instance", which clause
    /// 4.5.5 makes the instance carrying no `datasetId`.
    #[test]
    fn v1_0_reduces_a_multi_instance_attribute_to_its_default_instance() {
        let adapted = normalized(
            SpecVersion::V1_0,
            json!([{"type": "Property", "value": 25.0, "datasetId": "urn:ds:1"}, {"type": "Property", "value": 30.0}]),
        );
        pretty_assertions::assert_eq!(adapted, json!({"type": "Property", "value": 30.0}));
    }

    /// Notes 2 and 3: an attribute whose instances all carry a `datasetId` has
    /// no default instance, so the first one stands in; a 1.3 target keeps the
    /// array as it is.
    #[test]
    fn v1_0_takes_the_first_instance_when_no_default_exists_and_1_3_keeps_them_all() {
        let instances = json!([
            {"type": "Property", "value": 10, "datasetId": "urn:ngsi-ld:ds:1"},
            {"type": "Property", "value": 20, "datasetId": "urn:ngsi-ld:ds:2"}
        ]);
        pretty_assertions::assert_eq!(normalized(SpecVersion::V1_0, instances.clone()), json!({"type": "Property", "value": 10}));
        pretty_assertions::assert_eq!(normalized(SpecVersion::V1_3, instances).as_array().map(Vec::len), Some(2));
    }

    /// Clause 4.5.2.3 makes `type` optional in the concise representation, so
    /// the family is inferred from the value member. A concise
    /// `LanguageProperty` therefore reformats to a Property for a 1.3 target
    /// exactly as its normalized twin does, and collapses to the bare Property
    /// value the same clause mandates for a Property without sub-attributes.
    #[test]
    fn a_concise_language_property_reformats_and_collapses_for_a_1_3_target() {
        let adapted = concise(SpecVersion::V1_3, json!({"languageMap": {"en": "Tower", "de": "Turm"}}));
        pretty_assertions::assert_eq!(adapted, json!({"en": "Tower", "de": "Turm"}));
    }

    /// Table 4.3.6.8-2: `observedAt`, `unitCode`, `datasetId` and `valueType`
    /// go from a concise Property just as they go from a normalized one, and
    /// what is left is the clause 4.5.2.3 bare value.
    #[test]
    fn a_concise_property_loses_its_metadata_and_collapses_for_a_1_0_target() {
        let adapted = concise(
            SpecVersion::V1_0,
            json!({"value": 25.0, "observedAt": "2025-01-01T00:00:00Z", "unitCode": "CEL", "valueType": "Number"}),
        );
        pretty_assertions::assert_eq!(adapted, json!(25.0));
    }

    /// Clause 4.5.2.3 reads a bare object carrying a `type` member as a
    /// normalized attribute, and one carrying another family's value member as
    /// that family, so neither collapses: both keep the object form whose
    /// `value` member clause 4.5.2.3 makes Mandatory. An array of values reads
    /// back as one value and collapses.
    #[test]
    fn a_concise_value_that_would_be_misread_bare_keeps_its_object_form() {
        pretty_assertions::assert_eq!(concise(SpecVersion::V1_4, json!({"valueList": ["a", "b"]})), json!(["a", "b"]));
        pretty_assertions::assert_eq!(
            concise(SpecVersion::V1_4, json!({"json": {"type": "config", "level": 3}})),
            json!({"value": {"type": "config", "level": 3}})
        );
        pretty_assertions::assert_eq!(
            concise(SpecVersion::V1_4, json!({"json": {"object": "urn:ngsi-ld:X:1"}})),
            json!({"value": {"object": "urn:ngsi-ld:X:1"}})
        );
    }

    /// Clause 5.2.5: a concise Property's "value member cannot be a `GeoJSON`
    /// Object ... as it would be interpreted as a `GeoProperty`", so a value a
    /// reformat moved under `value` that is a geometry names the Property in
    /// `type`.
    #[test]
    fn a_reformatted_value_holding_a_geometry_keeps_the_property_type() {
        let geometry = json!({"type": "Point", "coordinates": [1.0, 2.0]});
        pretty_assertions::assert_eq!(
            concise(SpecVersion::V1_4, json!({"json": geometry})),
            json!({"type": "Property", "value": geometry})
        );
    }

    /// A concise `GeoProperty` left with nothing but its geometry collapses to
    /// it, as the concise rendering collapses one that never carried more.
    #[test]
    fn a_concise_geo_property_collapses_to_its_geometry_once_its_metadata_is_gone() {
        let geometry = json!({"type": "Point", "coordinates": [1.0, 2.0]});
        pretty_assertions::assert_eq!(
            concise(SpecVersion::V1_0, json!({"value": geometry, "observedAt": "2025-01-01T00:00:00Z"})),
            geometry
        );
    }

    /// Clause 4.5.2.3 writes several instances as "a JSON-LD array with such
    /// JSON-LD objects", so an instance among several keeps its object form
    /// when the fallbacks leave it nothing but its value.
    #[test]
    fn a_concise_instance_among_several_keeps_its_object_form() {
        pretty_assertions::assert_eq!(
            concise(
                SpecVersion::V1_4,
                json!([
                    {"value": 25.0, "datasetId": "urn:ds:1", "expiresAt": "2025-01-01T00:00:00Z"},
                    {"value": 30.0, "expiresAt": "2025-01-01T00:00:00Z"}
                ])
            ),
            json!([{"value": 25.0, "datasetId": "urn:ds:1"}, {"value": 30.0}])
        );
    }

    /// An array of plain objects is the value of one attribute, written bare,
    /// not instances of it.
    #[test]
    fn an_array_of_plain_objects_is_a_value_not_instances() {
        pretty_assertions::assert_eq!(concise(SpecVersion::V1_0, json!([{"a": 1}, {"a": 2}])), json!([{"a": 1}, {"a": 2}]));
    }

    /// A concise Relationship keeps its `object` member (clause 4.5.3.3) but
    /// still loses the members Table 4.3.6.8-3 removes.
    #[test]
    fn a_concise_relationship_loses_its_object_type_and_keeps_its_object() {
        let adapted = concise(
            SpecVersion::V1_0,
            json!({"object": "urn:ngsi-ld:Building:1", "objectType": "Building", "observedAt": "2025-01-01T00:00:00Z"}),
        );
        pretty_assertions::assert_eq!(adapted, json!({"object": "urn:ngsi-ld:Building:1"}));
    }

    /// A concise multi-instance attribute is an array of instances (clause
    /// 4.5.5), which Table 4.3.6.8-1 note 2 reduces to the default instance for
    /// a target below 1.3: the instance that carries no `datasetId`, which,
    /// standing alone now, collapses to its bare value (clause 4.5.2.3).
    #[test]
    fn a_concise_multi_instance_attribute_collapses_to_its_default_instance() {
        pretty_assertions::assert_eq!(
            concise(SpecVersion::V1_0, json!([{"value": 25.0, "datasetId": "urn:ds:1"}, {"value": 30.0}])),
            json!(30.0)
        );
        pretty_assertions::assert_eq!(
            concise(
                SpecVersion::V1_0,
                json!([{"value": 25.0, "datasetId": "urn:ds:1"}, {"value": 30.0, "observedAt": "2025-01-01T00:00:00Z"}])
            ),
            json!(30.0)
        );
    }

    /// Clause 4.5.5 separates the instances of one attribute by `datasetId`,
    /// which a bare value has nowhere to carry, so a concise Property whose
    /// value is an array of bare values keeps every element rather than being
    /// reduced to the first as a multi-instance attribute would be.
    #[test]
    fn a_concise_array_of_bare_values_is_one_attribute_value_and_not_several_instances() {
        pretty_assertions::assert_eq!(concise(SpecVersion::V1_0, json!(["a", "b", "c"])), json!(["a", "b", "c"]));
    }

    /// A `GeoProperty` value is a `GeoJSON` geometry whose `type` names a
    /// geometry rather than an attribute family, so the concise inference must
    /// not read it as an attribute and must leave it exactly as it is.
    #[test]
    fn a_concise_geo_property_value_is_not_read_as_an_attribute() {
        let geometry = json!({"type": "Point", "coordinates": [1.0, 2.0]});
        pretty_assertions::assert_eq!(concise(SpecVersion::V1_0, geometry.clone()), geometry);
    }
}
