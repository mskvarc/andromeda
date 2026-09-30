//! Notification attribute, dataset, and previous-value filtering.

use andromeda_entity::sys_attrs::expanded_entity_object_mut;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier};
use andromeda_ngsi_ld_model::expanded::attribute_change::AttributeChange;
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_ngsi_ld_vocabulary::{expanded::properties as expanded_properties, prefix};
use serde_json::Value;

pub fn filter_entity_attributes(entity: &Value, attrs: &[AttributeName]) -> Value {
    let Some(obj) = entity.as_object() else { return entity.clone() };

    let mut filtered = serde_json::Map::new();
    for (key, value) in obj {
        if key.starts_with('@') || attrs.iter().any(|a| a.as_str() == key) {
            filtered.insert(key.clone(), value.clone());
        }
    }
    Value::Object(filtered)
}

/// Keep only the Attribute instances a Subscription `datasetId` selects
/// (clause 5.8.6), `@none` selecting the default instance (Table 5.2.12-1),
/// and drop an Attribute left with none.
pub(super) fn filter_entity_by_dataset_id(entity: &mut Value, dataset_ids: &[DatasetSelector]) {
    let Some(obj) = expanded_entity_object_mut(entity) else { return };
    let default_context_prefix = format!("{}default-context/", prefix::NGSI_LD.as_str());

    let attr_keys: Vec<String> = obj
        .keys()
        .filter(|k| !k.starts_with('@') && (!k.starts_with(prefix::NGSI_LD.as_str()) || k.starts_with(&default_context_prefix)))
        .cloned()
        .collect();

    for key in attr_keys {
        let Some(arr) = obj.get_mut(&key).and_then(|v| v.as_array_mut()) else {
            continue;
        };

        let filtered: Vec<Value> = arr
            .drain(..)
            .filter(|instance| {
                let instance_dataset_id = instance
                    .as_object()
                    .and_then(|o| o.get(expanded_properties::DATASET_ID.as_str()))
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_object())
                    .and_then(|o| o.get("@id"))
                    .and_then(|v| v.as_str());

                // A stored `datasetId` was validated as an identifier when it was
                // written; one that no longer parses names no selectable dataset.
                match instance_dataset_id.map(str::parse::<Identifier>) {
                    None => dataset_ids.iter().any(|selector| selector.selects(None)),
                    Some(Ok(dataset_id)) => dataset_ids.iter().any(|selector| selector.selects(Some(&dataset_id))),
                    Some(Err(_)) => false,
                }
            })
            .collect();

        if filtered.is_empty() {
            obj.remove(&key);
        } else if let Some(entry) = obj.get_mut(&key) {
            *entry = Value::Array(filtered);
        }
    }
}

/// Which notified instances a `showChanges` notification reports the
/// previous value of (clause 5.8.6).
#[derive(Clone, Copy, Debug)]
pub(super) enum PreviousValueScope<'a> {
    /// Every instance of every Attribute: the Entity itself was deleted.
    EveryInstance,
    /// Exactly the instances the change touched.
    Instances(&'a [AttributeChange]),
}

/// Add the `showChanges` previous member to every notified instance `scope`
/// selects that the state before the change held.
pub(super) async fn inject_previous_values(current: &mut Value, previous: &Value, scope: PreviousValueScope<'_>, engine: &JsonLdEngine, context: &ContextSpec) {
    let (Some(curr_obj), Some(prev_obj)) = (current.as_object_mut(), previous.as_object()) else {
        return;
    };

    let keys: Vec<String> = curr_obj.keys().filter(|k| !k.starts_with('@') && *k != "id" && *k != "type").cloned().collect();

    let prepared_context = engine.prepare_term_context(context);
    for key in keys {
        let datasets: Option<Vec<Option<&str>>> = match scope {
            PreviousValueScope::EveryInstance => None,
            PreviousValueScope::Instances(instances) => {
                let expanded = engine
                    .expand_term(&key, &prepared_context)
                    .await
                    .ok()
                    .and_then(|iri| iri.as_str().parse::<AttributeName>().ok());
                let compact = key.parse::<AttributeName>().ok();
                let changed: Vec<Option<&str>> = instances
                    .iter()
                    .filter(|instance| Some(&instance.key.attr) == expanded.as_ref() || Some(&instance.key.attr) == compact.as_ref())
                    .map(|instance| instance.key.dataset_id.as_ref().map(Identifier::as_str))
                    .collect();
                if changed.is_empty() {
                    continue;
                }
                Some(changed)
            }
        };

        if let Some(prev_val) = prev_obj.get(&key)
            && let Some(curr_attr) = curr_obj.get_mut(&key)
        {
            inject_previous_for_dataset(curr_attr, prev_val, datasets.as_deref());
        }
    }
}

/// Add the previous member to each instance of one notified attribute whose
/// `datasetId` is among `datasets`, or to every instance when `datasets` is
/// `None`, from the previous instance of the same `datasetId`.
fn inject_previous_for_dataset(current: &mut Value, previous: &Value, datasets: Option<&[Option<&str>]>) {
    let previous_instances: Vec<&Value> = match previous {
        Value::Array(instances) => instances.iter().collect(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => vec![previous],
    };
    let inject = |current_instance: &mut Value| {
        let current_dataset_id = compact_dataset_id(current_instance);
        if datasets.is_some_and(|datasets| !datasets.contains(&current_dataset_id)) {
            return;
        }
        if let Some(previous_instance) = previous_instances.iter().find(|instance| compact_dataset_id(instance) == current_dataset_id) {
            inject_previous_into_instance(current_instance, previous_instance);
        }
    };
    match current {
        Value::Object(_) => inject(current),
        Value::Array(current_instances) => current_instances.iter_mut().for_each(inject),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Add the `showChanges` previous member to one notified instance.
///
/// The member's name is the family of the instance being notified, not of the
/// one it replaced: clause 5.6.19 lets a Replace Attribute change an
/// attribute's family, and each family's prohibitions are stated on itself --
/// clause 4.5.21.2 forbids `previousValue` on a `ListProperty` and clause
/// 4.5.2.2 forbids `previousValueList` on a Property. Taking the name from the
/// previous instance would write the departed family's member onto the new one,
/// which is prohibited on whichever of the two the change ended at.
///
/// When the previous instance carries no member of the current family -- which
/// is what a family change leaves behind -- there is no previous value in the
/// representation the clause requires, so none is reported rather than one
/// invented from the previous instance as a whole.
fn inject_previous_into_instance(current: &mut Value, previous: &Value) {
    let Some(current_object) = current.as_object_mut() else { return };
    let attribute_type = attribute_type_of(current_object.get("type"));
    let Some(previous_value) = extract_previous_value(previous, attribute_type) else {
        return;
    };
    current_object.insert(attribute_type.previous_key().to_owned(), previous_value);
}

fn compact_dataset_id(instance: &Value) -> Option<&str> {
    let object = instance.as_object()?;
    let value = object.get("datasetId").or_else(|| object.get(expanded_properties::DATASET_ID.as_str()))?;
    match value {
        Value::String(raw) => Some(raw),
        Value::Object(object) => object.get("id").or_else(|| object.get("@id")).and_then(Value::as_str),
        Value::Array(values) => values.first().and_then(|value| match value {
            Value::String(raw) => Some(raw.as_str()),
            Value::Object(object) => object.get("id").or_else(|| object.get("@id")).and_then(Value::as_str),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) => None,
        }),
        Value::Null | Value::Bool(_) | Value::Number(_) => None,
    }
}

/// The attribute family a compacted previous instance belongs to.
///
/// A previous representation reaches this after compaction from the expanded
/// form, so it declares its clause 5.2.4 type. An instance that declares none
/// is a bare value, which is the Property clause 4.5.2.3 abbreviates.
fn attribute_type_of(declared: Option<&Value>) -> AttributeType {
    declared
        .and_then(Value::as_str)
        .and_then(|token| token.parse::<AttributeType>().ok())
        .unwrap_or(AttributeType::Property)
}

/// The value the `showChanges` previous member carries for one instance.
///
/// Every family's previous member is "the same representation as" the value
/// member it shadows — clause 4.5.21.2 says so of `previousValueList` and
/// `valueList`, clause 4.5.22.2 of `previousObjectList` and `objectList` — so
/// the member to lift is the family's own, read through the one table that
/// holds them all. A `ListProperty` carries `valueList` and a
/// `ListRelationship` `objectList`, never the `value` those clauses prohibit,
/// so nothing here may fall back to a Property's member and emit the whole
/// previous instance in place of the previous array.
///
/// An instance carrying no value member at all is a concise bare value
/// (clause 4.5.2.3), which is its own previous value.
fn extract_previous_value(previous: &Value, attribute_type: AttributeType) -> Option<Value> {
    let Some(object) = previous.as_object() else {
        // A concise bare value is its own previous value (clause 4.5.2.3).
        return Some(previous.clone()); // clone: the notified instance owns the value the stored representation keeps.
    };
    // clone: same.
    object.get(attribute_type.compact_value_key()).cloned()
}

#[cfg(test)]
mod tests {
    use super::{filter_entity_by_dataset_id, inject_previous_for_dataset};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::dataset::DatasetSelector;
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    const SPEED: &str = "https://example.com/speed";
    const NAMED: &str = "urn:ngsi-ld:Dataset:named";

    /// An expanded entity whose `speed` holds a default and a named instance.
    fn two_instance_entity() -> Value {
        json!({
            "@id": "urn:ngsi-ld:Vehicle:1",
            SPEED: [
                {expanded_properties::VALUE.as_str(): [{"@value": 1}]},
                {expanded_properties::VALUE.as_str(): [{"@value": 2}], expanded_properties::DATASET_ID.as_str(): [{"@id": NAMED}]}
            ]
        })
    }

    fn selectors(raw: &[&str]) -> Vec<DatasetSelector> {
        raw.iter().map(|entry| entry.parse().expect("valid dataset selector")).collect()
    }

    /// Table 5.2.12-1: `@none` selects the default instance, and only it.
    #[test]
    fn the_none_selector_keeps_only_the_default_instance() {
        let mut entity = two_instance_entity();
        filter_entity_by_dataset_id(&mut entity, &selectors(&["@none"]));
        assert_eq!(entity[SPEED], json!([{expanded_properties::VALUE.as_str(): [{"@value": 1}]}]));
    }

    /// A named selector keeps the instance carrying that `datasetId` and drops
    /// the default one.
    #[test]
    fn a_named_selector_keeps_only_its_instance() {
        let mut entity = two_instance_entity();
        filter_entity_by_dataset_id(&mut entity, &selectors(&[NAMED]));
        let kept = entity[SPEED].as_array().expect("the named instance remains");
        assert_eq!(kept.len(), 1, "{entity}");
        assert_eq!(kept[0][expanded_properties::DATASET_ID.as_str()], json!([{"@id": NAMED}]));
    }

    /// `@none` beside a named selector keeps both instances.
    #[test]
    fn the_none_and_a_named_selector_together_keep_both_instances() {
        let mut entity = two_instance_entity();
        filter_entity_by_dataset_id(&mut entity, &selectors(&["@none", NAMED]));
        assert_eq!(entity[SPEED].as_array().map(Vec::len), Some(2), "{entity}");
    }

    /// An Attribute none of whose instances is selected leaves the entity.
    #[test]
    fn an_attribute_with_no_selected_instance_is_dropped() {
        let mut entity = two_instance_entity();
        filter_entity_by_dataset_id(&mut entity, &selectors(&["urn:ngsi-ld:Dataset:other"]));
        assert!(entity.get(SPEED).is_none(), "{entity}");
    }

    #[test]
    fn show_changes_marks_only_the_changed_dataset_instance() {
        let dataset = "urn:dataset:named".parse::<Identifier>().expect("valid dataset URI");
        let mut current = json!([
            {"type": "Property", "value": 10},
            {"type": "Property", "value": 20, "datasetId": dataset.as_str()}
        ]);
        let previous = json!([
            {"type": "Property", "value": 1},
            {"type": "Property", "value": 2, "datasetId": dataset.as_str()}
        ]);

        inject_previous_for_dataset(&mut current, &previous, Some(&[Some(dataset.as_str())]));

        assert!(current[0].get("previousValue").is_none());
        assert_eq!(current[1]["previousValue"], json!(2));
    }

    /// Clause 5.8.6 with clause 5.5.11.0: one change that updated two of an
    /// attribute's three instances reports the previous value of exactly those
    /// two in its one notification.
    #[test]
    fn show_changes_marks_every_instance_one_change_touched() {
        let mut current = json!([
            {"type": "Property", "value": 10},
            {"type": "Property", "value": 20, "datasetId": "urn:dataset:a"},
            {"type": "Property", "value": 30, "datasetId": "urn:dataset:b"}
        ]);
        let previous = json!([
            {"type": "Property", "value": 1},
            {"type": "Property", "value": 2, "datasetId": "urn:dataset:a"},
            {"type": "Property", "value": 3, "datasetId": "urn:dataset:b"}
        ]);

        inject_previous_for_dataset(&mut current, &previous, Some(&[None, Some("urn:dataset:b")]));

        assert_eq!(current[0]["previousValue"], json!(1));
        assert!(current[1].get("previousValue").is_none());
        assert_eq!(current[2]["previousValue"], json!(3));
    }

    /// Clause 4.5.21.2: `previousValueList` "represents the previous
    /// `ListProperty` `valueList` ... The representation is the same as that of
    /// `valueList`", and `value` / `previousValue` "shall never be present, as
    /// value is a generalization of `valueList`".
    #[test]
    fn show_changes_on_a_list_property_carries_the_previous_value_list_array() {
        let mut current = json!({"type": "ListProperty", "valueList": [3, 4]});
        let previous = json!({"type": "ListProperty", "valueList": [1, 2]});

        inject_previous_for_dataset(&mut current, &previous, None);

        assert_eq!(current["previousValueList"], json!([1, 2]));
        assert!(current.get("previousValue").is_none(), "{current}");
        assert!(current.get("value").is_none(), "{current}");
    }

    /// Clause 4.5.22.2: `previousObjectList` "represents the previous
    /// `ListRelationship` `objectList` ... The representation is the same as that
    /// of `objectList`", and `object` / `previousObject` "shall never be
    /// present as object is a generalization of `objectList`". The same clause
    /// makes that representation "an ordered array of Relationship Objects each
    /// consisting of a JSON object containing a single Attribute with a key
    /// called `object`", so the array lifted out of the previous instance is
    /// the array the previous instance held, member shapes included.
    #[test]
    fn show_changes_on_a_list_relationship_carries_the_previous_object_list_array() {
        let mut current = json!({"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Room:3"}]});
        let previous = json!({"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Room:1"}, {"object": "urn:ngsi-ld:Room:2"}]});

        inject_previous_for_dataset(&mut current, &previous, None);

        assert_eq!(
            current["previousObjectList"],
            json!([{"object": "urn:ngsi-ld:Room:1"}, {"object": "urn:ngsi-ld:Room:2"}])
        );
        assert!(current.get("previousObject").is_none(), "{current}");
        assert!(current.get("object").is_none(), "{current}");
    }

    /// Every family lifts its own value member into its own previous member
    /// (clauses 4.5.2.2, 4.5.3.2, 4.5.18.2, 4.5.20.2, 4.5.24.2), so no family
    /// falls back to another's and delivers the whole previous instance.
    #[test]
    fn every_family_lifts_its_own_value_member_into_its_own_previous_member() {
        let cases = [
            (json!({"type": "Property", "value": 1}), "previousValue", json!(1)),
            (
                json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}),
                "previousValue",
                json!({"type": "Point", "coordinates": [1.0, 2.0]}),
            ),
            (
                json!({"type": "Relationship", "object": "urn:ngsi-ld:Room:1"}),
                "previousObject",
                json!("urn:ngsi-ld:Room:1"),
            ),
            (
                json!({"type": "LanguageProperty", "languageMap": {"en": "yes"}}),
                "previousLanguageMap",
                json!({"en": "yes"}),
            ),
            (json!({"type": "VocabProperty", "vocab": "Male"}), "previousVocab", json!("Male")),
            (json!({"type": "JsonProperty", "json": {"id": "1"}}), "previousJson", json!({"id": "1"})),
            (json!({"type": "ListProperty", "valueList": [1, 2]}), "previousValueList", json!([1, 2])),
            (
                json!({"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Room:1"}]}),
                "previousObjectList",
                json!([{"object": "urn:ngsi-ld:Room:1"}]),
            ),
        ];
        for (previous, previous_member, expected) in cases {
            // clone: the case's previous instance is both the source and the shape the current one takes
            let mut current = previous.clone();
            inject_previous_for_dataset(&mut current, &previous, None);
            assert_eq!(current[previous_member], expected, "{previous}");
        }
    }

    #[test]
    fn an_entity_lifecycle_event_marks_every_dataset_instance() {
        let dataset = "urn:dataset:named".parse::<Identifier>().expect("valid dataset URI");
        let mut current = json!([
            {"type": "Property", "value": null},
            {"type": "Property", "value": null, "datasetId": dataset.as_str()}
        ]);
        let previous = json!([
            {"type": "Property", "value": 1},
            {"type": "Property", "value": 2, "datasetId": dataset.as_str()}
        ]);

        inject_previous_for_dataset(&mut current, &previous, None);

        assert_eq!(current[0]["previousValue"], json!(1));
        assert_eq!(current[1]["previousValue"], json!(2));
    }
}
