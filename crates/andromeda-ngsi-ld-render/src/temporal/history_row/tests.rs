//! The attribute instance each kind of history row renders as, pinned in the
//! expanded form the canonical codec writes it in.

use crate::temporal::history_row::neutral_row_to_instance;
use andromeda_ngsi_ld_entity::{document::instance_document::InstanceDocument, geometry_value::GeometryValue, non_empty::NonEmpty};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, instance_identity::InstanceIdentity, rfc3339::sub_second};
use andromeda_ngsi_ld_model::temporal::{TemporalHistoryRow, TemporalValue, observation_metadata::ObservationMetadata, structured_value::StructuredValue};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_vocabulary::{NULL_URI, expanded::properties as expanded_properties};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::{Value, json};
use uuid::Uuid;

/// The `DateTime` `@type` every rendered instant carries.
const DATE_TIME: &str = "https://uri.etsi.org/ngsi-ld/DateTime";

fn instant(epoch_seconds: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(epoch_seconds, 0).single().expect("valid instant")
}

/// An instant `seconds` past the epoch carrying `nanos` sub-second remainder.
fn instant_with_fraction(seconds: i64, nanos: u32) -> DateTime<Utc> {
    Utc.timestamp_opt(seconds, nanos).single().expect("valid instant")
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

/// A row of `family` holding `value`.
fn row_of(family: AttributeType, value: TemporalValue) -> TemporalHistoryRow {
    let mut row = row(value);
    row.attribute_type = family;
    row
}

fn one() -> TemporalValue {
    TemporalValue::Number(JsonNumber::from(1))
}

/// The expanded instance `row` renders as.
fn rendered(row: TemporalHistoryRow, sys_attrs: SysAttrsInclusion) -> Value {
    serde_json::to_value(neutral_row_to_instance(row, sys_attrs).expect("the row renders")).expect("the instance writes")
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
    let rendered = rendered(row(TemporalValue::Tombstone { deleted_at }), SysAttrsInclusion::Strip);
    pretty_assertions::assert_eq!(
        member(&rendered, AttributeType::Property.value_key_str()),
        Some(&json!([{"@value": NULL_URI.as_str()}]))
    );
    pretty_assertions::assert_eq!(
        member(&rendered, expanded_properties::DELETED_AT.as_str()),
        Some(&json!([{"@value": sub_second(deleted_at), "@type": DATE_TIME}]))
    );
}

/// Clause 5.2.21 admits only the four Temporal Properties, so the operational
/// write clock is absent from every rendering and a live observation carries no
/// `deletedAt` at all.
#[test]
fn the_operational_write_clock_is_never_rendered() {
    let row = row(one());
    let recorded_at = row.recorded_at;
    let rendered = rendered(row, SysAttrsInclusion::Include);
    pretty_assertions::assert_eq!(member(&rendered, expanded_properties::DELETED_AT.as_str()), None);
    assert!(
        !rendered.to_string().contains(&sub_second(recorded_at)),
        "recorded_at is not a Temporal Property and must not reach the rendering"
    );
}

/// Clause 4.11: an axis the observation does not carry is absent from the
/// rendering rather than filled in from another instant.
#[test]
fn an_axis_the_observation_does_not_carry_is_absent_from_the_rendering() {
    let mut row = row(one());
    row.observed_at = None;
    row.created_at = None;
    let rendered = rendered(row, SysAttrsInclusion::Include);
    pretty_assertions::assert_eq!(member(&rendered, expanded_properties::OBSERVED_AT.as_str()), None);
    pretty_assertions::assert_eq!(member(&rendered, expanded_properties::CREATED_AT.as_str()), None);
    assert!(member(&rendered, expanded_properties::MODIFIED_AT.as_str()).is_some());
}

/// A broker-minted identifier renders through the permanently stable template
/// of its family, on live observations and tombstones alike.
#[test]
fn a_generated_identifier_renders_through_the_permanent_template() {
    let expected = json!([{"@id": "urn:ngsi-ld:attribute:instance:01942f6c-7f00-7000-8000-000000000001"}]);
    let live = rendered(row(one()), SysAttrsInclusion::Strip);
    let deleted = rendered(
        row(TemporalValue::Tombstone {
            deleted_at: instant(1_700_000_300),
        }),
        SysAttrsInclusion::Strip,
    );
    pretty_assertions::assert_eq!(member(&live, expanded_properties::INSTANCE_ID.as_str()), Some(&expected));
    pretty_assertions::assert_eq!(member(&deleted, expanded_properties::INSTANCE_ID.as_str()), Some(&expected));
}

/// Clauses 4.5.7 and 4.5.8: a deleted instance renders the null member its own
/// family prescribes — `object` for a Relationship, the single-null array of
/// clause 4.5.0 in `valueList` / `objectList` for the two list families of
/// clauses 4.5.21.2 and 4.5.22.2, the family's value member otherwise — never a
/// Property value the instance never had.
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
        let rendered = rendered(row_of(attribute_type, TemporalValue::Tombstone { deleted_at }), SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(member(&rendered, attribute_type.value_key_str()), Some(&expected), "{attribute_type}");
        pretty_assertions::assert_eq!(member(&rendered, "@type"), Some(&json!([attribute_type.iri().as_str()])));
    }
}

/// Clauses 4.5.21.2 and 4.5.22.2: a live list observation renders as the
/// ordered `@list` its member is a container for; a `ListRelationship` member
/// is the normalized `object`-keyed node Table 5.2.37-1 reserves to the
/// normalized form.
#[test]
fn a_list_observation_renders_as_the_ordered_list_its_member_contains() {
    let list = StructuredValue::from_stored(
        AttributeType::ListProperty,
        &"https://example.org/speed".parse().expect("valid attribute name"),
        r#"[{"@value": "first"}, {"@value": "second"}]"#,
    )
    .expect("a stored list");
    let rendered_list = rendered(row_of(AttributeType::ListProperty, TemporalValue::Structured(list)), SysAttrsInclusion::Strip);
    pretty_assertions::assert_eq!(
        member(&rendered_list, AttributeType::ListProperty.value_key_str()),
        Some(&json!([{"@list": [{"@value": "first"}, {"@value": "second"}]}]))
    );

    let targets = StructuredValue::ObjectList(vec![
        "urn:ngsi-ld:Room:1".parse().expect("a valid URI"),
        "urn:ngsi-ld:Room:2".parse().expect("a valid URI"),
    ]);
    let rendered_targets = rendered(
        row_of(AttributeType::ListRelationship, TemporalValue::Structured(targets)),
        SysAttrsInclusion::Strip,
    );
    let object = expanded_properties::OBJECT.as_str();
    pretty_assertions::assert_eq!(
        member(&rendered_targets, AttributeType::ListRelationship.value_key_str()),
        Some(&json!([{"@list": [
            {object: [{"@id": "urn:ngsi-ld:Room:1"}]},
            {object: [{"@id": "urn:ngsi-ld:Room:2"}]},
        ]}]))
    );
}

/// Clause 4.5.3.2: `object` is "the Relationship's object represented by a URI
/// or array of URIs", so every target renders as its own identifier node.
#[test]
fn a_relationship_renders_one_identifier_node_per_target() {
    let several = StructuredValue::Objects(
        NonEmpty::try_from_vec(vec![
            "urn:ngsi-ld:Device:14142".parse().expect("a valid URI"),
            "urn:ngsi-ld:Device:13562".parse().expect("a valid URI"),
        ])
        .expect("two targets"),
    );
    let rendered_several = rendered(
        row_of(AttributeType::Relationship, TemporalValue::Structured(several)),
        SysAttrsInclusion::Strip,
    );
    pretty_assertions::assert_eq!(
        member(&rendered_several, AttributeType::Relationship.value_key_str()),
        Some(&json!([{"@id": "urn:ngsi-ld:Device:14142"}, {"@id": "urn:ngsi-ld:Device:13562"}]))
    );
    let single = rendered(
        row_of(
            AttributeType::Relationship,
            TemporalValue::Uri("urn:ngsi-ld:Device:14142".parse().expect("a valid URI")),
        ),
        SysAttrsInclusion::Strip,
    );
    pretty_assertions::assert_eq!(
        member(&single, AttributeType::Relationship.value_key_str()),
        Some(&json!([{"@id": "urn:ngsi-ld:Device:14142"}]))
    );
}

/// A number renders in the exact shape it was stored in.
#[test]
fn a_number_renders_in_the_shape_it_was_stored_in() {
    for raw in ["1.0", "9007199254740993", "-0.00025"] {
        let number = raw.parse::<JsonNumber>().expect("an exact number");
        let expected: Value = serde_json::from_str(&format!(r#"[{{"@value": {number}}}]"#)).expect("the expected node parses");
        let rendered = rendered(row(TemporalValue::Number(number)), SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(member(&rendered, AttributeType::Property.value_key_str()), Some(&expected), "{raw}");
    }
}

/// Clause 5.2.5 puts no restriction on a Property `value`: an array-valued
/// Property renders as the several value nodes it was stored as, an object
/// element as the node it was stored as.
#[test]
fn a_structured_property_renders_the_values_it_was_stored_as() {
    let attribute = "https://example.org/speed".parse().expect("valid attribute name");
    for (stored, expected) in [
        (json!(["a", "b"]), json!([{"@value": "a"}, {"@value": "b"}])),
        (json!([1, 2, 3]), json!([{"@value": 1}, {"@value": 2}, {"@value": 3}])),
        (
            json!([1, {"https://uri.etsi.org/ngsi-ld/default-context/zone": [{"@value": "Asia/Dubai"}]}]),
            json!([{"@value": 1}, {"https://uri.etsi.org/ngsi-ld/default-context/zone": [{"@value": "Asia/Dubai"}]}]),
        ),
        (
            json!({"https://uri.etsi.org/ngsi-ld/default-context/unit": [{"@value": "km"}]}),
            json!([{"https://uri.etsi.org/ngsi-ld/default-context/unit": [{"@value": "km"}]}]),
        ),
    ] {
        let value = StructuredValue::from_stored(AttributeType::Property, &attribute, &stored.to_string()).expect("a stored value");
        let rendered = rendered(row(TemporalValue::Structured(value)), SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(member(&rendered, AttributeType::Property.value_key_str()), Some(&expected), "{stored}");
    }
}

/// A geometry observation renders as the expanded `GeoJSON` node JSON-LD
/// expansion makes of a geometry under the core context: the type under the
/// `GeoJSON` vocabulary and the coordinates as nested lists of value nodes,
/// altitude included.
#[test]
fn a_history_geometry_renders_the_expanded_geojson_node() {
    let polygon = json!({"type": "Polygon", "coordinates": [[[0.0, 0.0, 12.5], [4.0, 0.0, 12.5], [4.0, 4.0, 12.5], [0.0, 0.0, 12.5]]]});
    let geometry = serde_json::from_value::<Geometry>(polygon).expect("a polygon");
    let rendered = rendered(
        row_of(AttributeType::GeoProperty, TemporalValue::Geometry(GeometryValue::new(geometry.into_parts().0))),
        SysAttrsInclusion::Strip,
    );
    let position = |ordinates: &[f64]| json!({"@list": ordinates.iter().map(|ordinate| json!({"@value": ordinate})).collect::<Vec<_>>()});
    let expected = json!([{
        "@type": ["https://purl.org/geojson/vocab#Polygon"],
        "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@list": [
            position(&[0.0, 0.0, 12.5]),
            position(&[4.0, 0.0, 12.5]),
            position(&[4.0, 4.0, 12.5]),
            position(&[0.0, 0.0, 12.5])
        ]}]}]
    }]);
    let rendered_geometry = member(&rendered, AttributeType::GeoProperty.value_key_str()).expect("the geometry is rendered");
    pretty_assertions::assert_eq!(
        serde_json::from_value::<Geometry>(compacted_geojson(rendered_geometry)).expect("the rendering reads back"),
        serde_json::from_value::<Geometry>(compacted_geojson(&expected)).expect("the expected node reads back"),
    );
}

/// The `GeoJSON` object an expanded geometry node spells, read naively for the
/// comparison above (ordinates compare by value, not by spelling).
fn compacted_geojson(expanded: &Value) -> Value {
    let node = &expanded[0];
    let kind = node["@type"][0].as_str().and_then(|iri| iri.rsplit('#').next()).unwrap_or_default();
    json!({"type": kind, "coordinates": unlist(&node["https://purl.org/geojson/vocab#coordinates"][0])})
}

/// A nested `@list` of value nodes as the plain nested array it stands for.
fn unlist(list: &Value) -> Value {
    match list.get("@list").and_then(Value::as_array) {
        Some(items) => Value::Array(items.iter().map(unlist).collect()),
        None => list.get("@value").cloned().unwrap_or(Value::Null), // clone: the comparison owns the ordinate
    }
}

/// A client-supplied identifier renders in the exact spelling it arrived in,
/// which clause 4.5.7 requires the broker to hand back unchanged.
#[test]
fn a_verbatim_identifier_renders_in_the_spelling_it_arrived_in() {
    let mut row = row(one());
    row.instance = InstanceIdentity::Verbatim("https://example.org/instances/1".parse().expect("valid URI"));
    let rendered = rendered(row, SysAttrsInclusion::Strip);
    pretty_assertions::assert_eq!(
        member(&rendered, expanded_properties::INSTANCE_ID.as_str()),
        Some(&json!([{"@id": "https://example.org/instances/1"}]))
    );
}

/// A reified scope renders as a Property carrying no `instanceId` or
/// `datasetId` (clauses 4.5.4 and 4.5.6).
#[test]
fn a_scope_instance_carries_no_identity_members() {
    let mut scope = row(TemporalValue::Scopes(vec!["/Madrid".parse().expect("a scope path")]));
    scope.attribute_name = expanded_properties::SCOPE.as_str().parse().expect("valid attribute name");
    scope.dataset_id = Some("urn:ngsi-ld:Dataset:a".parse().expect("a valid identifier"));
    let rendered = rendered(scope, SysAttrsInclusion::Strip);
    pretty_assertions::assert_eq!(member(&rendered, expanded_properties::INSTANCE_ID.as_str()), None);
    pretty_assertions::assert_eq!(member(&rendered, expanded_properties::DATASET_ID.as_str()), None);
    pretty_assertions::assert_eq!(
        member(&rendered, AttributeType::Property.value_key_str()),
        Some(&json!([{"@value": "/Madrid"}]))
    );
}

/// Clauses 5.7.3/5.7.4 with 4.6.3: an `observedAt` rebuilt from the stored
/// instant keeps its sub-second precision, and a whole-second one renders as a
/// bare `Z`, which is the spelling a client that wrote a whole second reads
/// back.
#[test]
fn observed_at_keeps_exactly_the_sub_second_precision_it_carries() {
    for (nanos, expected) in [(123_000_000, "2023-11-14T22:13:20.123Z"), (0, "2023-11-14T22:13:20Z")] {
        let mut row = row(one());
        row.observed_at = Some(instant_with_fraction(1_700_000_000, nanos));
        let rendered = rendered(row, SysAttrsInclusion::Strip);
        pretty_assertions::assert_eq!(
            member(&rendered, expanded_properties::OBSERVED_AT.as_str()),
            Some(&json!([{"@value": expected, "@type": DATE_TIME}]))
        );
    }
}

/// A `DateTime`-typed value keeps its sub-second precision too.
#[test]
fn a_datetime_value_keeps_its_sub_second_precision() {
    let rendered = rendered(
        row(TemporalValue::DateTime(instant_with_fraction(1_700_000_000, 456_000_000))),
        SysAttrsInclusion::Strip,
    );
    pretty_assertions::assert_eq!(
        member(&rendered, AttributeType::Property.value_key_str()),
        Some(&json!([{"@value": "2023-11-14T22:13:20.456Z", "@type": DATE_TIME}]))
    );
}

/// The broker-minted `createdAt`/`modifiedAt` axes render at the microsecond
/// resolution clause 4.8 system times are stored at, exactly as the current
/// representation of the same entity renders them.
#[test]
fn broker_minted_system_times_render_at_their_stored_resolution() {
    let mut row = row(one());
    row.created_at = Some(instant_with_fraction(1_700_000_100, 999_000_000));
    row.modified_at = Some(instant_with_fraction(1_700_000_200, 999_000_000));
    let rendered = rendered(row, SysAttrsInclusion::Include);
    pretty_assertions::assert_eq!(
        member(&rendered, expanded_properties::CREATED_AT.as_str()),
        Some(&json!([{"@value": "2023-11-14T22:15:00.999000Z", "@type": DATE_TIME}]))
    );
    pretty_assertions::assert_eq!(
        member(&rendered, expanded_properties::MODIFIED_AT.as_str()),
        Some(&json!([{"@value": "2023-11-14T22:16:40.999000Z", "@type": DATE_TIME}]))
    );
}

/// The stored metadata renders beside the value, in its own members: it can
/// never stand in for the value member it is not.
#[test]
fn the_stored_metadata_renders_beside_the_value() {
    let mut row = row(one());
    let members = json!({
        "@type": [AttributeType::Property.iri_str()],
        "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "KMH"}],
        "https://example.org/accuracy": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 0.5}]}]
    });
    let document = InstanceDocument::from_slice(&row.attribute_name, &serde_json::to_vec(&members).expect("JSON")).expect("expanded members");
    row.metadata = ObservationMetadata::from_parts(document.into_parts());
    let rendered = rendered(row, SysAttrsInclusion::Strip);
    pretty_assertions::assert_eq!(member(&rendered, expanded_properties::UNIT_CODE.as_str()), Some(&json!([{"@value": "KMH"}])));
    pretty_assertions::assert_eq!(member(&rendered, AttributeType::Property.value_key_str()), Some(&json!([{"@value": 1}])));
    assert!(member(&rendered, "https://example.org/accuracy").is_some());
}
