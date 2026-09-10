//! Projecting one backend-neutral temporal observation into an expanded attribute instance.

use crate::temporal::geojson_ld::expand_geojson;
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, instance_family::InstanceFamily, rfc3339::whole_seconds};
use andromeda_ngsi_ld_model::{
    expanded::null_sentinel::expanded_null_value,
    temporal::{TemporalHistoryRow, TemporalValue},
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_vocabulary::expanded::{classes as expanded_classes, properties as expanded_properties};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};

/// The observed time span of a set of history rows.
pub type TimeRange = (DateTime<Utc>, DateTime<Utc>);

/// Build an expanded NGSI-LD attribute instance from a backend-neutral
/// [`TemporalHistoryRow`].
///
/// The row already carries decoded, strongly typed values. The only work left is the JSON-LD structural expansion that the specification prescribes.
#[must_use]
pub fn neutral_row_to_expanded_instance(row: &TemporalHistoryRow, sys_attrs: SysAttrsInclusion) -> Value {
    let mut instance = Map::new();
    instance.insert("@type".to_string(), json!([row.attribute_type.iri().as_str()]));

    // A tombstone carries no observation of its own: its value member, its
    // deletion instant and its identity are the whole rendering.
    if let TemporalValue::Tombstone { deleted_at } = &row.value {
        insert_deleted_value(&mut instance, row);
        instance.insert(expanded_properties::DELETED_AT.as_str().to_string(), expanded_datetime(*deleted_at));
        insert_identity_members(&mut instance, row);
        return Value::Object(instance);
    }

    insert_observed_value(&mut instance, row);
    insert_temporal_properties(&mut instance, row, sys_attrs);
    insert_identity_members(&mut instance, row);
    insert_stored_sub_attributes(&mut instance, row);

    Value::Object(instance)
}

/// The expanded node an NGSI-LD `DateTime` renders as.
fn expanded_datetime(instant: DateTime<Utc>) -> Value {
    json!([{"@value": whole_seconds(instant), "@type": expanded_classes::DATE_TIME.as_str()}])
}

/// Insert the value member of a deleted instance.
///
/// Clauses 4.5.7 and 4.5.8: a deleted instance renders the `urn:ngsi-ld:null`
/// value for its attribute family beside its `deletedAt`. The null URI is
/// synthesised here from the tombstone itself; storage never holds it, so no
/// client value spelling it can be mistaken for a deletion. Which node each
/// family takes is clause 4.5.0's answer, shared with the deletion a patch
/// writes so the two can never disagree: a deleted `ListProperty` or
/// `ListRelationship` renders the single-null array of clauses 4.5.21.2 and
/// 4.5.22.2 in its own list member, never the `value` those clauses prohibit.
fn insert_deleted_value(instance: &mut Map<String, Value>, row: &TemporalHistoryRow) {
    instance.insert(row.attribute_type.value_key_str().to_string(), expanded_null_value(row.attribute_type));
}

/// Insert the value member of a live observation, in the expanded shape the
/// attribute family the row belongs to prescribes.
fn insert_observed_value(instance: &mut Map<String, Value>, row: &TemporalHistoryRow) {
    let expanded = match &row.value {
        TemporalValue::Number(number) => json!([{"@value": number.to_json_value()}]),
        TemporalValue::String(text) => {
            if row.attribute_type == AttributeType::VocabProperty {
                json!([{"@id": text}])
            } else {
                json!([{"@value": text}])
            }
        }
        TemporalValue::Boolean(flag) => json!([{"@value": flag}]),
        TemporalValue::Geometry(geometry) => json!([expand_geojson(geometry)]),
        TemporalValue::Object(object) => expanded_structured_value(object, row.attribute_type),
        TemporalValue::DateTime(instant) => expanded_datetime(*instant),
        TemporalValue::Uri(uri) => json!([{"@id": uri}]),
        // Clause 4.18: "The value of scope is represented as a JSON array in
        // case there is more than one Scope". A multi-valued string is several
        // value nodes in the expanded form — a JSON-LD value node cannot hold
        // an array — and compaction gives back the bare string for one Scope
        // and the array for several, which is what annex C clause C.5.16
        // renders for Vehicles B9211 and A8311 respectively.
        TemporalValue::Scopes(paths) => Value::Array(paths.iter().map(|path| json!({"@value": path.as_str()})).collect()),
        // A deleted instance renders through `insert_deleted_value` instead,
        // before this is ever reached.
        TemporalValue::Tombstone { .. } => return,
    };
    instance.insert(row.attribute_type.value_key_str().to_string(), expanded);
}

/// The expanded shape a structured value renders in, which the attribute family
/// decides: a `JsonProperty` keeps its JSON literal, a `LanguageProperty` is
/// stored already expanded, a `VocabProperty` names one or more vocabulary
/// members by identifier, a Relationship with more than one target names each
/// of them by identifier (clause 4.5.3.2's "URI or array of URIs"), and the two
/// list families of clauses 4.5.21 and 4.5.22 render an ordered `@list`, which
/// is the container the core context gives `valueList` and `objectList` and the
/// only shape that compacts back to the ordered array those clauses mandate.
///
/// A `ListProperty` stores its members already expanded, so the list is its own
/// rendering; a `ListRelationship` stores the bare target URIs its `objectList`
/// names and rebuilds each of them into the normalized member clause 4.5.22.2
/// mandates.
fn expanded_structured_value(object: &Value, attribute_type: AttributeType) -> Value {
    match attribute_type {
        AttributeType::JsonProperty => json!([{"@value": object, "@type": "@json"}]),
        // clone: the rendered instance owns the expanded language map the row holds.
        AttributeType::LanguageProperty => object.clone(),
        AttributeType::VocabProperty => object.as_array().map_or_else(
            || json!([{"@id": object}]),
            |members| Value::Array(members.iter().map(|member| json!({"@id": member})).collect()),
        ),
        // Clause 4.5.3.2 defines `object` as "the Relationship's object
        // represented by a URI or array of URIs", so a stored multi-target
        // Relationship renders one identifier node per target and compacts back
        // to the array of URIs the clause names.
        AttributeType::Relationship => object.as_array().map_or_else(
            || json!([{"@id": object}]),
            |targets| Value::Array(targets.iter().map(|target| json!({"@id": target})).collect()),
        ),
        AttributeType::ListProperty => json!([{"@list": object}]),
        AttributeType::ListRelationship => object.as_array().map_or_else(
            || json!([{"@list": [expanded_object_list_member(object)]}]),
            |targets| json!([{"@list": targets.iter().map(expanded_object_list_member).collect::<Vec<Value>>()}]),
        ),
        AttributeType::Property | AttributeType::GeoProperty => expanded_property_value(object),
    }
}

/// The expanded shape a structured Property or `GeoProperty` value renders in.
///
/// Clause 5.2.5 puts no restriction on a Property `value`, so an array and a
/// JSON object are both admissible, and a JSON-LD value node holds neither: it
/// admits a string, number, boolean or null and nothing else. The write path
/// stores what JSON-LD expansion made of the value, and that is what decides
/// the reading here. `crates/andromeda-temporal/src/attribute_instance_value.rs`
/// stores an array-valued Property as the elements of the several value nodes
/// expansion produced — which is why `["a"]` is stored as the string `"a"` and
/// `[["a"],["b","c"]]` as `["a","b","c"]` — so the array renders back as those
/// value nodes and compacts to the array the current representation of the same
/// Attribute answers with. An object-valued Property is stored as the node
/// object expansion produced, so it renders as that node.
fn expanded_property_value(object: &Value) -> Value {
    match object {
        Value::Array(elements) => Value::Array(elements.iter().map(|element| json!({"@value": element})).collect()),
        // clone: the rendered instance owns the expanded node the row holds.
        Value::Object(_) => Value::Array(vec![object.clone()]),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => json!([{"@value": object}]),
    }
}

/// One member of a normalized `objectList`, in expanded form.
///
/// Clause 4.5.22.2, Mandatory: "`objectList`: a JSON representation of an
/// ordered array of Relationship Objects each consisting of a JSON object
/// containing a single Attribute with a key called `object` and its value
/// holding the Relationship's object represented by a URI." Table 5.2.37-1
/// spells out the split the two representations make of that array — "In the
/// normalized form, each array element holds a JSON object containing a single
/// Attribute with a key called `object` ... In the concise form, each string in
/// the array holds a valid URI" — and a temporal retrieval answers in the
/// normalized form, so the member is built as the `object`-keyed node that
/// compacts to `{"object": "urn:..."}` rather than as the literal that would
/// compact to the bare URI of the concise form.
fn expanded_object_list_member(target: &Value) -> Value {
    let mut member = Map::new();
    member.insert(expanded_properties::OBJECT.as_str().to_owned(), json!([{"@id": target}]));
    Value::Object(member)
}

/// Insert the Temporal Properties the observation carries.
///
/// Each of these renders from its own instant alone (clause 5.2.21): one that
/// the observation does not carry is absent from the rendering rather than
/// filled in from the operational write clock, which is not a Temporal Property
/// at all.
fn insert_temporal_properties(instance: &mut Map<String, Value>, row: &TemporalHistoryRow, sys_attrs: SysAttrsInclusion) {
    if let Some(observed_at) = row.observed_at {
        instance.insert(expanded_properties::OBSERVED_AT.as_str().to_string(), expanded_datetime(observed_at));
    }

    match sys_attrs {
        SysAttrsInclusion::Include => {
            if let Some(created_at) = row.created_at {
                instance.insert(expanded_properties::CREATED_AT.as_str().to_string(), expanded_datetime(created_at));
            }
            if let Some(modified_at) = row.modified_at {
                instance.insert(expanded_properties::MODIFIED_AT.as_str().to_string(), expanded_datetime(modified_at));
            }
        }
        SysAttrsInclusion::Strip => {}
    }
}

/// Insert the members that identify the instance: the `instanceId` clause 4.5.7
/// hands back, and the `datasetId` it belongs to when it names one.
fn insert_identity_members(instance: &mut Map<String, Value>, row: &TemporalHistoryRow) {
    let family = InstanceFamily::of_attribute_name(&row.attribute_name);
    instance.insert(
        expanded_properties::INSTANCE_ID.as_str().to_string(),
        json!([{"@id": row.instance.rendered(family).to_string()}]),
    );

    if let Some(dataset_id) = &row.dataset_id {
        instance.insert(expanded_properties::DATASET_ID.as_str().to_string(), json!([{"@id": dataset_id.as_str()}]));
    }
}

/// Merge the already-expanded sub-attributes the row stored beside its value.
fn insert_stored_sub_attributes(instance: &mut Map<String, Value>, row: &TemporalHistoryRow) {
    if let Some(metadata) = &row.metadata
        && let Some(members) = metadata.as_object()
    {
        for (key, value) in members {
            // clone: the rendered instance owns the members read out of the row.
            instance.insert(key.clone(), value.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal::history_row::neutral_row_to_expanded_instance;
    use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, instance_identity::InstanceIdentity, rfc3339::whole_seconds};
    use andromeda_ngsi_ld_model::temporal::{TemporalHistoryRow, TemporalValue};
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_ngsi_ld_vocabulary::{NULL_URI, expanded::properties as expanded_properties};
    use chrono::{DateTime, TimeZone, Utc};
    use serde_json::{Value, json};
    use uuid::Uuid;

    fn instant(epoch_seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(epoch_seconds, 0).single().expect("valid instant")
    }

    /// The UUID every fixture instance is minted from.
    fn minted() -> Uuid {
        Uuid::from_u128(0x0194_2f6c_7f00_7000_8000_0000_0000_0001)
    }

    /// A live observation whose four axes are all distinct instants.
    fn row(value: TemporalValue) -> TemporalHistoryRow {
        TemporalHistoryRow {
            attribute_name: "https://example.org/speed".parse().expect("valid attribute name"),
            attribute_type: AttributeType::Property,
            dataset_id: None,
            value,
            observed_at: Some(instant(1_700_000_000)),
            recorded_at: instant(1_700_000_900),
            created_at: Some(instant(1_700_000_100)),
            modified_at: Some(instant(1_700_000_200)),
            instance: InstanceIdentity::Generated(minted()),
            metadata: None,
            partition_total: None,
        }
    }

    fn member<'a>(rendered: &'a Value, key: &str) -> Option<&'a Value> {
        rendered.as_object().and_then(|instance| instance.get(key))
    }

    /// Clauses 4.5.7 and 4.5.8: a deleted instance renders `urn:ngsi-ld:null`
    /// beside its `deletedAt`, and that value is synthesised from the tombstone
    /// rather than read back out of storage.
    #[test]
    fn a_tombstone_renders_the_synthesised_null_uri_and_its_deletion_instant() {
        let deleted_at = instant(1_700_000_300);
        let rendered = neutral_row_to_expanded_instance(&row(TemporalValue::Tombstone { deleted_at }), SysAttrsInclusion::Strip);

        pretty_assertions::assert_eq!(
            member(&rendered, AttributeType::Property.value_key_str()),
            Some(&json!([{"@value": NULL_URI.as_str()}]))
        );
        pretty_assertions::assert_eq!(
            member(&rendered, expanded_properties::DELETED_AT.as_str()),
            Some(&json!([{"@value": whole_seconds(deleted_at), "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}]))
        );
    }

    /// Clause 5.2.21 admits only the four Temporal Properties, so the
    /// operational write clock is absent from every rendering and a live
    /// observation carries no `deletedAt` at all.
    #[test]
    fn the_operational_write_clock_is_never_rendered() {
        let row = row(TemporalValue::Number(JsonNumber::try_from(1.0).expect("a finite number")));
        let rendered = neutral_row_to_expanded_instance(&row, SysAttrsInclusion::Include);

        pretty_assertions::assert_eq!(member(&rendered, expanded_properties::DELETED_AT.as_str()), None);
        assert!(
            !rendered.to_string().contains(&whole_seconds(row.recorded_at)),
            "recorded_at is not a Temporal Property and must not reach the rendering"
        );
    }

    /// Clause 4.11: an axis the observation does not carry is absent from the
    /// rendering rather than filled in from another instant.
    #[test]
    fn an_axis_the_observation_does_not_carry_is_absent_from_the_rendering() {
        let mut row = row(TemporalValue::Number(JsonNumber::try_from(1.0).expect("a finite number")));
        row.observed_at = None;
        row.created_at = None;
        let rendered = neutral_row_to_expanded_instance(&row, SysAttrsInclusion::Include);

        pretty_assertions::assert_eq!(member(&rendered, expanded_properties::OBSERVED_AT.as_str()), None);
        pretty_assertions::assert_eq!(member(&rendered, expanded_properties::CREATED_AT.as_str()), None);
        assert!(member(&rendered, expanded_properties::MODIFIED_AT.as_str()).is_some());
    }

    /// A broker-minted identifier renders through the permanently stable
    /// template of its family, on live observations and tombstones alike.
    #[test]
    fn a_generated_identifier_renders_through_the_permanent_template() {
        let expected = json!([{"@id": "urn:ngsi-ld:attribute:instance:01942f6c-7f00-7000-8000-000000000001"}]);
        let live = neutral_row_to_expanded_instance(
            &row(TemporalValue::Number(JsonNumber::try_from(1.0).expect("a finite number"))),
            SysAttrsInclusion::Strip,
        );
        let deleted = neutral_row_to_expanded_instance(
            &row(TemporalValue::Tombstone {
                deleted_at: instant(1_700_000_300),
            }),
            SysAttrsInclusion::Strip,
        );

        pretty_assertions::assert_eq!(member(&live, expanded_properties::INSTANCE_ID.as_str()), Some(&expected));
        pretty_assertions::assert_eq!(member(&deleted, expanded_properties::INSTANCE_ID.as_str()), Some(&expected));
    }

    /// Clauses 4.5.7 and 4.5.8: a deleted instance renders the null member its
    /// own family prescribes — `object` for a Relationship, the single-null
    /// array of clause 4.5.0 in `valueList` / `objectList` for the two list
    /// families of clauses 4.5.21.2 and 4.5.22.2, the family's value member
    /// otherwise — never a Property value the instance never had.
    #[test]
    fn a_tombstone_renders_the_null_member_of_the_family_the_instance_belonged_to() {
        let deleted_at = instant(1_700_000_300);
        let null_uri = NULL_URI.as_str();
        let families = [
            (AttributeType::Relationship, json!([{"@id": null_uri}])),
            (AttributeType::Property, json!([{"@value": null_uri}])),
            (AttributeType::LanguageProperty, json!([{"@value": null_uri}])),
            (AttributeType::GeoProperty, json!([{"@value": null_uri}])),
            (AttributeType::VocabProperty, json!([{"@id": null_uri}])),
            (AttributeType::JsonProperty, json!([{"@value": null_uri, "@type": "@json"}])),
            (AttributeType::ListProperty, json!([{"@list": [{"@value": null_uri}]}])),
            (AttributeType::ListRelationship, json!([{"@list": [{"@value": null_uri}]}])),
        ];
        for (attribute_type, expected) in families {
            let mut row = row(TemporalValue::Tombstone { deleted_at });
            row.attribute_type = attribute_type;
            let rendered = neutral_row_to_expanded_instance(&row, SysAttrsInclusion::Strip);
            pretty_assertions::assert_eq!(member(&rendered, attribute_type.value_key_str()), Some(&expected), "{attribute_type}");
            pretty_assertions::assert_eq!(member(&rendered, "@type"), Some(&json!([attribute_type.iri().as_str()])));
        }
    }

    /// Clauses 4.5.21.2 and 4.5.22.2: a live list observation renders as the
    /// ordered `@list` its member is a container for, so compaction gives back
    /// the ordered array those clauses mandate rather than one value node
    /// holding an array.
    #[test]
    fn a_list_observation_renders_as_the_ordered_list_its_member_contains() {
        let mut list_property = row(TemporalValue::Object(json!([{"@value": "first"}, {"@value": "second"}])));
        list_property.attribute_type = AttributeType::ListProperty;
        let rendered = neutral_row_to_expanded_instance(&list_property, SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(
            member(&rendered, AttributeType::ListProperty.value_key_str()),
            Some(&json!([{"@list": [{"@value": "first"}, {"@value": "second"}]}]))
        );
    }

    /// Clause 4.5.22.2, Mandatory: each `objectList` element is "a JSON object
    /// containing a single Attribute with a key called `object` and its value
    /// holding the Relationship's object represented by a URI", which Table
    /// 5.2.37-1 reserves to the normalized form. The expanded member is
    /// therefore an `object`-keyed identifier node, never the literal that
    /// compacts to the bare URI of the concise form.
    #[test]
    fn a_list_relationship_renders_each_member_as_the_normalized_object_node() {
        let mut list_relationship = row(TemporalValue::Object(json!(["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"])));
        list_relationship.attribute_type = AttributeType::ListRelationship;
        let rendered = neutral_row_to_expanded_instance(&list_relationship, SysAttrsInclusion::Strip);
        let object = expanded_properties::OBJECT.as_str();
        pretty_assertions::assert_eq!(
            member(&rendered, AttributeType::ListRelationship.value_key_str()),
            Some(&json!([{"@list": [
                {object: [{"@id": "urn:ngsi-ld:Room:1"}]},
                {object: [{"@id": "urn:ngsi-ld:Room:2"}]},
            ]}]))
        );
    }

    /// Clause 4.5.3.2: `object` is "the Relationship's object represented by a
    /// URI or array of URIs", so every target of a stored multi-target
    /// Relationship renders as its own identifier node and none is dropped.
    #[test]
    fn a_multi_target_relationship_renders_one_identifier_node_per_target() {
        let mut relationship = row(TemporalValue::Object(json!(["urn:ngsi-ld:Device:14142", "urn:ngsi-ld:Device:13562"])));
        relationship.attribute_type = AttributeType::Relationship;
        let rendered = neutral_row_to_expanded_instance(&relationship, SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(
            member(&rendered, AttributeType::Relationship.value_key_str()),
            Some(&json!([{"@id": "urn:ngsi-ld:Device:14142"}, {"@id": "urn:ngsi-ld:Device:13562"}]))
        );

        let mut single = row(TemporalValue::Uri("urn:ngsi-ld:Device:14142".parse().expect("a valid URI")));
        single.attribute_type = AttributeType::Relationship;
        let rendered = neutral_row_to_expanded_instance(&single, SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(
            member(&rendered, AttributeType::Relationship.value_key_str()),
            Some(&json!([{"@id": "urn:ngsi-ld:Device:14142"}]))
        );
    }

    /// A whole-valued double renders as the double it is: reshaping it into an
    /// integer would make the temporal representation disagree with the current
    /// one for the same stored value.
    #[test]
    fn a_number_renders_in_the_shape_it_was_stored_in() {
        let cases = [
            (JsonNumber::try_from(1.0).expect("a finite number"), json!(1)),
            (JsonNumber::from(9_007_199_254_740_993_i64), json!(9_007_199_254_740_993_i64)),
            (JsonNumber::try_from(-2.5e-4).expect("a finite number"), json!(-2.5e-4)),
        ];
        for (number, expected) in cases {
            let rendered = neutral_row_to_expanded_instance(&row(TemporalValue::Number(number)), SysAttrsInclusion::Strip);
            pretty_assertions::assert_eq!(member(&rendered, AttributeType::Property.value_key_str()), Some(&json!([{"@value": expected}])));
        }
    }

    /// Clause 5.2.5 puts no restriction on a Property `value`, and a JSON-LD
    /// value node holds a string, number, boolean or null and nothing else, so
    /// an array-valued Property renders as the several value nodes its stored
    /// elements came from. One node holding the array would be a document no
    /// compaction could read.
    #[test]
    fn an_array_valued_property_renders_one_value_node_per_element() {
        let cases = [
            (json!(["a", "b"]), json!([{"@value": "a"}, {"@value": "b"}])),
            (json!([1, 2, 3]), json!([{"@value": 1}, {"@value": 2}, {"@value": 3}])),
            (json!([true, false]), json!([{"@value": true}, {"@value": false}])),
        ];
        for (stored, expected) in cases {
            let rendered = neutral_row_to_expanded_instance(&row(TemporalValue::Object(stored)), SysAttrsInclusion::Strip);
            pretty_assertions::assert_eq!(member(&rendered, AttributeType::Property.value_key_str()), Some(&expected));
        }
    }

    /// The same holds for a `GeoProperty` whose stored value is structured
    /// rather than a geometry: the two families share one value reading.
    #[test]
    fn an_array_valued_geo_property_renders_the_same_value_nodes() {
        let mut geo = row(TemporalValue::Object(json!(["a", "b"])));
        geo.attribute_type = AttributeType::GeoProperty;
        let rendered = neutral_row_to_expanded_instance(&geo, SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(
            member(&rendered, AttributeType::GeoProperty.value_key_str()),
            Some(&json!([{"@value": "a"}, {"@value": "b"}]))
        );
    }

    /// An object-valued Property is stored as the node object JSON-LD expansion
    /// produced, so it renders as that node rather than being wrapped in a
    /// value node that could not hold it.
    #[test]
    fn an_object_valued_property_renders_the_expanded_node_it_was_stored_as() {
        let node = json!({"https://uri.etsi.org/ngsi-ld/default-context/unit": [{"@value": "km"}]});
        // clone: the fixture keeps its own copy of the node it asserts on.
        let rendered = neutral_row_to_expanded_instance(&row(TemporalValue::Object(node.clone())), SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(member(&rendered, AttributeType::Property.value_key_str()), Some(&json!([node])));
    }

    /// A scalar keeps the single value node it always had, so the array and
    /// object readings are additions rather than a change of the common case.
    #[test]
    fn a_scalar_structured_value_still_renders_one_value_node() {
        let rendered = neutral_row_to_expanded_instance(&row(TemporalValue::Object(json!("free"))), SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(member(&rendered, AttributeType::Property.value_key_str()), Some(&json!([{"@value": "free"}])));
    }

    /// A client-supplied identifier renders in the exact spelling it arrived
    /// in, which clause 4.5.7 requires the broker to hand back unchanged.
    #[test]
    fn a_verbatim_identifier_renders_in_the_spelling_it_arrived_in() {
        let mut row = row(TemporalValue::Number(JsonNumber::try_from(1.0).expect("a finite number")));
        row.instance = InstanceIdentity::Verbatim("https://example.org/instances/1".parse().expect("valid URI"));
        let rendered = neutral_row_to_expanded_instance(&row, SysAttrsInclusion::Strip);

        pretty_assertions::assert_eq!(
            member(&rendered, expanded_properties::INSTANCE_ID.as_str()),
            Some(&json!([{"@id": "https://example.org/instances/1"}]))
        );
    }
}
