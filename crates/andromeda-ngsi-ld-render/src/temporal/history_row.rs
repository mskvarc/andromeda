//! Projecting one backend-neutral temporal observation into an expanded attribute instance.

use crate::temporal::geojson_ld::expand_geojson;
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, instance_family::InstanceFamily, rfc3339::whole_seconds};
use andromeda_ngsi_ld_model::{
    attribute::json_number::JsonNumber,
    temporal::{TemporalHistoryRow, TemporalValue},
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_vocabulary::{
    NULL_URI,
    expanded::{classes as expanded_classes, properties as expanded_properties},
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

/// The observed time span of a set of history rows.
pub type TimeRange = (DateTime<Utc>, DateTime<Utc>);

/// Build an expanded NGSI-LD attribute instance from a backend-neutral
/// [`TemporalHistoryRow`].
///
/// The row already carries decoded, strongly typed values. The only work left is the JSON-LD structural expansion that the specification prescribes.
pub fn neutral_row_to_expanded_instance(row: &TemporalHistoryRow, sys_attrs: SysAttrsInclusion) -> Value {
    let mut instance = serde_json::Map::new();
    let family = InstanceFamily::of_attribute_name(&row.attribute_name);

    instance.insert("@type".to_string(), json!([row.attribute_type.iri().as_str()]));

    let value_key = row.attribute_type.value_key_str();

    // Clauses 4.5.7 and 4.5.8: a deleted instance renders the `urn:ngsi-ld:null`
    // value for its attribute family beside its `deletedAt`. The null URI is
    // synthesised here from the tombstone itself; storage never holds it, so no
    // client value spelling it can be mistaken for a deletion.
    if let TemporalValue::Tombstone { deleted_at } = &row.value {
        // Clause 4.18 reserves `urn:ngsi-ld:null` for a deleted scope, and a
        // LanguageProperty's `@none` key expands to a bare `@value` node — the
        // language-map convention the expansion engine writes and reads, since
        // `@none` is not a well-formed language tag (JSON-LD 1.1 clause 9.5).
        if row.attribute_type == AttributeType::Relationship {
            instance.insert(value_key.to_string(), json!([{"@id": NULL_URI.as_str()}]));
        } else {
            instance.insert(value_key.to_string(), json!([{"@value": NULL_URI.as_str()}]));
        }
        instance.insert(
            expanded_properties::DELETED_AT.as_str().to_string(),
            json!([{"@value": whole_seconds(*deleted_at), "@type": expanded_classes::DATE_TIME.as_str()}]),
        );
        instance.insert(
            expanded_properties::INSTANCE_ID.as_str().to_string(),
            json!([{"@id": row.instance.rendered(family).to_string()}]),
        );
        if let Some(ds_id) = &row.dataset_id {
            instance.insert(expanded_properties::DATASET_ID.as_str().to_string(), json!([{"@id": ds_id.as_str()}]));
        }
        return Value::Object(instance);
    }

    match &row.value {
        // The number renders in the shape it was stored in. Re-deciding here
        // whether a value is "really" an integer would make the temporal
        // representation of a value disagree with the current one.
        TemporalValue::Number(v) => {
            let json_val = match v {
                JsonNumber::Signed(value) => json!(value),
                JsonNumber::Unsigned(value) => json!(value),
                JsonNumber::Double(value) => json!(value),
            };
            instance.insert(value_key.to_string(), json!([{"@value": json_val}]));
        }
        TemporalValue::String(v) => {
            if row.attribute_type == AttributeType::VocabProperty {
                instance.insert(value_key.to_string(), json!([{"@id": v}]));
            } else {
                instance.insert(value_key.to_string(), json!([{"@value": v}]));
            }
        }
        TemporalValue::Boolean(v) => {
            instance.insert(value_key.to_string(), json!([{"@value": v}]));
        }
        TemporalValue::Geometry(geo) => {
            let expanded = expand_geojson(geo);
            instance.insert(value_key.to_string(), json!([expanded]));
        }
        TemporalValue::Object(v) => {
            if row.attribute_type == AttributeType::JsonProperty {
                instance.insert(value_key.to_string(), json!([{"@value": v, "@type": "@json"}]));
            } else if row.attribute_type == AttributeType::LanguageProperty {
                instance.insert(value_key.to_string(), v.clone());
            } else if row.attribute_type == AttributeType::VocabProperty {
                if let Some(arr) = v.as_array() {
                    let nodes: Vec<Value> = arr.iter().map(|val| json!({"@id": val})).collect();
                    instance.insert(value_key.to_string(), Value::Array(nodes));
                } else {
                    instance.insert(value_key.to_string(), json!([{"@id": v}]));
                }
            } else {
                instance.insert(value_key.to_string(), json!([{"@value": v}]));
            }
        }
        TemporalValue::DateTime(v) => {
            instance.insert(
                value_key.to_string(),
                json!([{"@value": whole_seconds(*v), "@type": expanded_classes::DATE_TIME.as_str()}]),
            );
        }
        TemporalValue::Uri(v) => {
            instance.insert(value_key.to_string(), json!([{"@id": v}]));
        }
        TemporalValue::Tombstone { .. } => {} // handled above
    }

    if let Some(observed_at) = row.observed_at {
        instance.insert(
            expanded_properties::OBSERVED_AT.as_str().to_string(),
            json!([{"@value": whole_seconds(observed_at), "@type": expanded_classes::DATE_TIME.as_str()}]),
        );
    }

    // Each of the remaining Temporal Properties renders from its own instant
    // alone (clause 5.2.21): one that the observation does not carry is absent
    // from the rendering rather than filled in from the operational write
    // clock, which is not a Temporal Property at all.
    if sys_attrs == SysAttrsInclusion::Include {
        if let Some(created_at) = row.created_at {
            instance.insert(
                expanded_properties::CREATED_AT.as_str().to_string(),
                json!([{"@value": whole_seconds(created_at), "@type": expanded_classes::DATE_TIME.as_str()}]),
            );
        }
        if let Some(modified_at) = row.modified_at {
            instance.insert(
                expanded_properties::MODIFIED_AT.as_str().to_string(),
                json!([{"@value": whole_seconds(modified_at), "@type": expanded_classes::DATE_TIME.as_str()}]),
            );
        }
    }

    instance.insert(
        expanded_properties::INSTANCE_ID.as_str().to_string(),
        json!([{"@id": row.instance.rendered(family).to_string()}]),
    );

    if let Some(ds_id) = &row.dataset_id {
        instance.insert(expanded_properties::DATASET_ID.as_str().to_string(), json!([{"@id": ds_id.as_str()}]));
    }

    if let Some(meta) = &row.metadata
        && let Some(obj) = meta.as_object()
    {
        for (k, v) in obj {
            instance.insert(k.clone(), v.clone());
        }
    }

    Value::Object(instance)
}

#[cfg(test)]
mod tests {
    use crate::temporal::history_row::neutral_row_to_expanded_instance;
    use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, instance_identity::InstanceIdentity, rfc3339::whole_seconds};
    use andromeda_ngsi_ld_model::{
        attribute::json_number::JsonNumber,
        temporal::{TemporalHistoryRow, TemporalValue},
    };
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
        let row = row(TemporalValue::Number(JsonNumber::Double(1.0)));
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
        let mut row = row(TemporalValue::Number(JsonNumber::Double(1.0)));
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
        let live = neutral_row_to_expanded_instance(&row(TemporalValue::Number(JsonNumber::Double(1.0))), SysAttrsInclusion::Strip);
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
    /// own family prescribes — `object` for a Relationship, the family's value
    /// member otherwise — never a Property value the instance never had.
    #[test]
    fn a_tombstone_renders_the_null_member_of_the_family_the_instance_belonged_to() {
        let deleted_at = instant(1_700_000_300);
        let null_uri = NULL_URI.as_str();
        let families = [
            (AttributeType::Relationship, json!([{"@id": null_uri}])),
            (AttributeType::Property, json!([{"@value": null_uri}])),
            (AttributeType::LanguageProperty, json!([{"@value": null_uri}])),
            (AttributeType::GeoProperty, json!([{"@value": null_uri}])),
            (AttributeType::VocabProperty, json!([{"@value": null_uri}])),
            (AttributeType::JsonProperty, json!([{"@value": null_uri}])),
        ];
        for (attribute_type, expected) in families {
            let mut row = row(TemporalValue::Tombstone { deleted_at });
            row.attribute_type = attribute_type;
            let rendered = neutral_row_to_expanded_instance(&row, SysAttrsInclusion::Strip);
            pretty_assertions::assert_eq!(member(&rendered, attribute_type.value_key_str()), Some(&expected), "{attribute_type}");
            pretty_assertions::assert_eq!(member(&rendered, "@type"), Some(&json!([attribute_type.iri().as_str()])));
        }
    }

    /// A whole-valued double renders as the double it is: reshaping it into an
    /// integer would make the temporal representation disagree with the current
    /// one for the same stored value.
    #[test]
    fn a_number_renders_in_the_shape_it_was_stored_in() {
        let cases = [
            (JsonNumber::Double(1.0), json!(1.0)),
            (JsonNumber::Signed(9_007_199_254_740_993), json!(9_007_199_254_740_993_i64)),
            (JsonNumber::Double(-2.5e-4), json!(-2.5e-4)),
        ];
        for (number, expected) in cases {
            let rendered = neutral_row_to_expanded_instance(&row(TemporalValue::Number(number)), SysAttrsInclusion::Strip);
            pretty_assertions::assert_eq!(member(&rendered, AttributeType::Property.value_key_str()), Some(&json!([{"@value": expected}])));
        }
    }

    /// A client-supplied identifier renders in the exact spelling it arrived
    /// in, which clause 4.5.7 requires the broker to hand back unchanged.
    #[test]
    fn a_verbatim_identifier_renders_in_the_spelling_it_arrived_in() {
        let mut row = row(TemporalValue::Number(JsonNumber::Double(1.0)));
        row.instance = InstanceIdentity::Verbatim("https://example.org/instances/1".parse().expect("valid URI"));
        let rendered = neutral_row_to_expanded_instance(&row, SysAttrsInclusion::Strip);

        pretty_assertions::assert_eq!(
            member(&rendered, expanded_properties::INSTANCE_ID.as_str()),
            Some(&json!([{"@id": "https://example.org/instances/1"}]))
        );
    }
}
