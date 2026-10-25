//! Flattening a committed state into rows and assembling the rows give back
//! the same state, for every family at every depth; assembly refuses rows that
//! break the stored layout's invariants.

use crate::current_attribute::{
    assemble::assemble,
    flatten::flatten,
    row::{CurrentAttributeRow, row_column},
};
use andromeda_ngsi_ld_entity::{
    attributes::Attributes,
    entity_state::EntityState,
    expanded_value::ExpandedValue,
    family_value::FamilyValue,
    instance::Instance,
    literal::{Literal, LiteralValue},
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    entity_type_name::EntityTypeName,
    reference::NgsiLdReference,
    time::system_instant::SystemInstant,
};
use andromeda_ngsi_ld_model::temporal::structured_value::StructuredValue;
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_postgresql_attribute_instance::{
    assemble::AssemblyError,
    flatten::FlattenError,
    name_keys::{HeldKeys, KeyNames, MentionedNames, NameKeys},
    stored_value::{LiteralAnnotation, StoredValue},
    sub_attribute::{SubAttributeElement, SubAttributeRecord},
};
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, value_kind::ValueKind};
use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, dictionary::resolved_names::ResolvedNames, entity_type_name_key::EntityTypeNameKey};
use bigdecimal::BigDecimal;
use chrono::{TimeZone, Utc};
use indexmap::IndexSet;
use sea_query::Iden;
use serde_json::{Value, json};
use std::collections::BTreeSet;

const NGSI: &str = "https://uri.etsi.org/ngsi-ld/";
const GEOJSON: &str = "https://purl.org/geojson/vocab#";
const EX: &str = "https://ex.org/";

/// The expanded IRI of an NGSI-LD core term.
pub(crate) fn ngsi(term: &str) -> String {
    format!("{NGSI}{term}")
}

/// The expanded IRI of an example term.
pub(crate) fn ex(term: &str) -> String {
    format!("{EX}{term}")
}

/// The expanded IRI of an NGSI-LD attribute family.
pub(crate) fn family(term: &str) -> Value {
    json!([ngsi(term)])
}

/// A typed `DateTime` member.
pub(crate) fn date_time(text: &str) -> Value {
    json!([{"@type": ngsi("DateTime"), "@value": text}])
}

/// `instance` with the members of `extra` added.
pub(crate) fn with(mut instance: Value, extra: &Value) -> Value {
    if let (Some(instance), Some(extra)) = (instance.as_object_mut(), extra.as_object()) {
        instance.extend(extra.iter().map(|(key, value)| (key.clone(), value.clone())));
    }
    instance
}

/// A Property instance holding the expanded `values`.
pub(crate) fn property(values: &Value) -> Value {
    json!({"@type": family("Property"), ngsi("hasValue"): values})
}

/// A geometry node: a Point at `coordinates`, with `extra` members.
fn point(coordinates: &Value, extra: &Value) -> Value {
    with(
        json!({"@type": [format!("{GEOJSON}Point")], format!("{GEOJSON}coordinates"): [{"@list": coordinates}]}),
        extra,
    )
}

/// The `bbox` and foreign member of a geometry.
fn geometry_members() -> Value {
    json!({
        format!("{GEOJSON}bbox"): [{"@list": [{"@value": 1.5}, {"@value": 2.5}, {"@value": 3.5}, {"@value": 4.5}]}],
        ex("title"): [{"@value": "a point"}]
    })
}

/// One instance of each of the eight families, every optional member set,
/// each carrying `sub_attributes`.
pub(crate) fn every_family(sub_attributes: &Value) -> Value {
    let geo = point(&json!([{"@value": 13.5}, {"@value": 52.25}]), &geometry_members());
    json!({
        ex("speed"): [with(with(property(&json!([{"@value": 55.5}])), &json!({
            ngsi("unitCode"): [{"@value": "KMH"}],
            ngsi("hasValueType"): [{"@id": ex("Speed")}],
            ngsi("observedAt"): date_time("2026-01-01T00:00:00.500Z"),
            ngsi("expiresAt"): date_time("2027-01-01T00:00:00Z"),
            ngsi("datasetId"): [{"@id": "urn:ngsi-ld:Dataset:a"}]
        })), sub_attributes)],
        ex("location"): [with(json!({
            "@type": family("GeoProperty"),
            ngsi("hasValue"): [geo],
            ngsi("unitCode"): [{"@value": "MTR"}],
            ngsi("hasValueType"): [{"@id": ex("Location")}],
            ngsi("observedAt"): date_time("2026-01-01T00:00:01Z")
        }), sub_attributes)],
        ex("owner"): [with(json!({
            "@type": family("Relationship"),
            ngsi("hasObject"): [{"@id": "urn:ngsi-ld:Person:2"}, {"@id": "urn:ngsi-ld:Person:1"}],
            ngsi("hasObjectType"): [{"@id": ex("Person")}, {"@id": ex("Agent")}],
            ngsi("observedAt"): date_time("2026-01-01T00:00:02Z")
        }), sub_attributes)],
        ex("name"): [with(json!({
            "@type": family("LanguageProperty"),
            ngsi("hasLanguageMap"): [{"@value": "Bahnhof", "@language": "de"}, {"@value": "Station", "@language": "en"}, {"@value": "Gare"}],
            ngsi("hasValueType"): [{"@id": ex("Name")}]
        }), sub_attributes)],
        ex("category"): [with(json!({
            "@type": family("VocabProperty"),
            ngsi("hasVocab"): [{"@id": ex("commercial")}, {"@id": ex("office")}],
            ngsi("hasValueType"): [{"@id": ex("Category")}]
        }), sub_attributes)],
        ex("config"): [with(json!({
            "@type": family("JsonProperty"),
            ngsi("hasJSON"): [{"@type": "@json", "@value": {"a": [1, 2.5, {"b": null}], "c": "text"}}],
            ngsi("hasValueType"): [{"@id": ex("Config")}]
        }), sub_attributes)],
        ex("readings"): [with(json!({
            "@type": family("ListProperty"),
            ngsi("hasValueList"): [{"@list": [{"@value": 1}, {"@value": "two"}, {"@list": [{"@value": true}]}]}],
            ngsi("unitCode"): [{"@value": "CEL"}],
            ngsi("hasValueType"): [{"@id": ex("Readings")}]
        }), sub_attributes)],
        ex("route"): [with(json!({
            "@type": family("ListRelationship"),
            ngsi("hasObjectList"): [{"@list": [{ngsi("hasObject"): [{"@id": "urn:ngsi-ld:Stop:2"}]}, {ngsi("hasObject"): [{"@id": "urn:ngsi-ld:Stop:1"}]}]}],
            ngsi("hasObjectType"): [{"@id": ex("Stop")}]
        }), sub_attributes)],
        ngsi("ngsildproof"): [with(json!({
            "@type": family("Property"),
            ngsi("hasValue"): [{"@value": "proof"}],
            ngsi("entityIdSealed"): [{"@value": "urn:ngsi-ld:Building:1"}],
            ngsi("entityTypeSealed"): [{"@value": "Building"}, {"@value": "Shop"}]
        }), sub_attributes)]
    })
}

/// One sub-attribute of each of the eight families, each holding one
/// sub-attribute of the same family.
pub(crate) fn nested_every_family() -> Value {
    let inner = every_family(&json!({}));
    let mut nested = serde_json::Map::new();
    if let Some(families) = inner.as_object() {
        // Only an `ngsildproof` attribute carries seals (clause 4.5.2.2), so it
        // is not renamed into a sub-attribute here.
        for (name, instances) in families.iter().filter(|(name, _)| **name != ngsi("ngsildproof")) {
            let child = json!({format!("{name}Inner"): instances});
            let outer = instances
                .as_array()
                .and_then(|instances| instances.first())
                .map_or(Value::Null, |instance| with(instance.clone(), &child));
            nested.insert(format!("{name}Sub"), json!([outer]));
        }
    }
    Value::Object(nested)
}

/// The stored document of an Entity with `attributes`.
pub(crate) fn document(attributes: &Value) -> Value {
    json!([with(json!({"@id": "urn:ngsi-ld:Building:1", "@type": [ex("Building")]}), attributes)])
}

/// The write instant every fixture is committed at.
fn now() -> SystemInstant {
    SystemInstant::new(Utc.with_ymd_and_hms(2026, 10, 9, 12, 0, 0).single().expect("a valid instant"))
}

/// The state a create commits from the stored `document`.
pub(crate) fn committed(document: &Value) -> EntityState {
    committed_text(&document.to_string())
}

/// The state a create commits from the stored document `text`.
fn committed_text(text: &str) -> EntityState {
    let mut state = EntityState::from_slice(text.as_bytes()).expect("a stored entity");
    state.settle(None, now());
    state
}

/// Dictionary keys for every name `state` mentions, both ways: the names a
/// write interns ([`MentionedNames`]), which flattening every fixture proves
/// complete.
pub(crate) fn dictionary(state: &EntityState) -> (NameKeys, KeyNames) {
    dictionary_of([state])
}

/// Dictionary keys for every name `states` mention, their entity types
/// included, both ways.
pub(crate) fn dictionary_of<'a>(states: impl IntoIterator<Item = &'a EntityState>) -> (NameKeys, KeyNames) {
    let mut mentioned = MentionedNames::default();
    for state in states {
        mentioned.collect(state.attributes());
        mentioned.types.extend(state.types().as_slice().iter().cloned());
    }
    let names: IndexSet<AttributeName> = mentioned.attributes.into_iter().collect();
    let types: IndexSet<EntityTypeName> = mentioned.types.into_iter().collect();
    let attribute_keys = names.iter().zip(1..).map(|(name, key)| (name.clone(), AttributeNameKey::from(key)));
    let type_keys = types.iter().zip(1..).map(|(name, key)| (name.clone(), EntityTypeNameKey::from(key)));
    let keys = NameKeys::new(ResolvedNames::from_pairs(attribute_keys.clone()), ResolvedNames::from_pairs(type_keys.clone()));
    let key_names = KeyNames::from_pairs(
        attribute_keys.map(|(name, key)| (key, name)),
        type_keys.map(|(name, key)| (key, name.as_str().parse::<NgsiLdReference>().expect("a type IRI"))),
    );
    (keys, key_names)
}

/// The rows of `state`, every element passed through its composite record.
fn rows(state: &EntityState, keys: &NameKeys) -> Vec<CurrentAttributeRow> {
    flatten(state.attributes(), keys)
        .expect("a committed state flattens")
        .into_iter()
        .map(|mut row| {
            row.sub_attributes = row
                .sub_attributes
                .into_iter()
                .map(|element| SubAttributeElement::try_from(SubAttributeRecord::try_from(element).expect("the element encodes")).expect("the record decodes"))
                .collect();
            row
        })
        .collect()
}

/// Flatten `state`, assemble its rows, and assert the attributes come back;
/// the rows are returned for the caller to inspect.
fn round_trip(state: &EntityState) -> Vec<CurrentAttributeRow> {
    let (keys, names) = dictionary(state);
    let assembled = assemble(rows(state, &keys), &names).expect("the rows assemble");
    pretty_assertions::assert_eq!(&assembled, state.attributes());
    rows(state, &keys)
}

/// The value the single row of `rows` holds.
fn only_value(rows: &[CurrentAttributeRow]) -> &StoredValue {
    match rows {
        [row] => &row.members.value,
        other => panic!("one row expected, got {}", other.len()),
    }
}

/// The stored JSON of an object value.
fn object_text(value: &StoredValue) -> &str {
    let StoredValue::Object(json) = value else {
        panic!("an object value expected, got {value:?}");
    };
    json.get()
}

/// The instance `name` holds, by `datasetId`.
fn held<'a>(attributes: &'a Attributes, name: &str, dataset_id: Option<&str>) -> &'a Instance {
    let dataset_id = dataset_id.map(|dataset_id| dataset_id.parse().expect("a datasetId"));
    attributes
        .get(name)
        .and_then(|instances| instances.get(dataset_id.as_ref()))
        .expect("the instance")
}

#[test]
fn every_family_round_trips() {
    let state = committed(&document(&every_family(&nested_every_family())));
    let rows = round_trip(&state);
    pretty_assertions::assert_eq!(rows.len(), 9);
    for row in &rows {
        pretty_assertions::assert_eq!(row.sub_attributes.len(), 16, "eight families, each with its child");
        assert!(row.sub_attributes.iter().any(|element| element.parent > 0), "a second level");
    }
}

/// A read looks up the names of the keys its rows hold, which are the keys of
/// exactly the names the write interned, at every depth.
#[test]
fn the_keys_rows_hold_are_the_keys_of_the_names_a_write_mentions() {
    let state = committed(&document(&every_family(&nested_every_family())));
    let (keys, _) = dictionary(&state);
    let mut held = HeldKeys::default();
    for row in rows(&state, &keys) {
        held.collect(&row.members, &row.sub_attributes);
    }
    let mut mentioned = MentionedNames::default();
    mentioned.collect(state.attributes());
    let expected_attributes: BTreeSet<i32> = mentioned.attributes.iter().map(|name| keys.attribute(name).expect("a key").get()).collect();
    let expected_types: BTreeSet<i32> = mentioned.types.iter().map(|name| keys.entity_type(name).expect("a key").get()).collect();
    pretty_assertions::assert_eq!(held.attributes.iter().map(|key| key.get()).collect::<BTreeSet<i32>>(), expected_attributes);
    pretty_assertions::assert_eq!(held.types.iter().map(|key| key.get()).collect::<BTreeSet<i32>>(), expected_types);
    assert!(!expected_types.is_empty(), "the fixture holds datatypes, value types and object types");
}

#[test]
fn every_family_round_trips_as_a_sub_attribute() {
    let state = committed(&document(
        &json!({ex("holder"): [with(property(&json!([{"@value": 1}])), &every_family(&json!({})))]}),
    ));
    let rows = round_trip(&state);
    let [row] = rows.as_slice() else { panic!("one row") };
    pretty_assertions::assert_eq!(row.sub_attributes.iter().map(|element| element.parent).collect::<Vec<_>>(), vec![0; 9]);
}

#[test]
fn a_number_round_trips_exactly() {
    let numbers = ["123456789012345678901234567890.000000000000000001", "0", "-5", "1e2"];
    for number in numbers {
        let text = format!(
            r#"[{{"@id": "urn:ngsi-ld:Building:1", "@type": ["{EX}Building"], "{EX}n": [{{"@type": ["{NGSI}Property"], "{NGSI}hasValue": [{{"@value": {number}}}], "{EX}sub": [{{"@type": ["{NGSI}Property"], "{NGSI}hasValue": [{{"@value": {number}}}]}}]}}]}}]"#
        );
        let state = committed_text(&text);
        let rows = round_trip(&state);
        let expected: JsonNumber = number.parse().expect("a JSON number");
        let StoredValue::Number { number: stored, .. } = only_value(&rows) else {
            panic!("a number value")
        };
        pretty_assertions::assert_eq!(stored, &expected);
        let [element] = rows[0].sub_attributes.as_slice() else {
            panic!("one element")
        };
        pretty_assertions::assert_eq!(
            element.members.value,
            StoredValue::Number {
                number: expected,
                datatype: None
            }
        );
    }
    let hundred = committed_text(&format!(
        r#"[{{"@id": "urn:ngsi-ld:Building:1", "@type": ["{EX}Building"], "{EX}n": [{{"@type": ["{NGSI}Property"], "{NGSI}hasValue": [{{"@value": 1e2}}]}}]}}]"#
    ));
    let hundred_rows = round_trip(&hundred);
    let StoredValue::Number { number, .. } = only_value(&hundred_rows) else {
        panic!("a number value")
    };
    pretty_assertions::assert_eq!(number.decimal(), &BigDecimal::from(100));
}

#[test]
fn a_datetime_value_renders_its_sub_second_form() {
    let state = committed(&document(
        &json!({ex("at"): [property(&json!([{"@type": ngsi("DateTime"), "@value": "2024-01-01T00:00:00.5Z"}]))]}),
    ));
    let rows = round_trip(&state);
    assert!(matches!(only_value(&rows), StoredValue::DateTime { datatype: Some(_), .. }));
    let FamilyValue::Property { values, .. } = held(state.attributes(), &ex("at"), None).value() else {
        panic!("a Property")
    };
    let [ExpandedValue::Literal(literal)] = values.as_slice() else {
        panic!("one literal")
    };
    pretty_assertions::assert_eq!(literal.value(), &LiteralValue::String("2024-01-01T00:00:00.500Z".into()));
}

/// Annex C.6: a string declared as no time value is a String, held as written.
#[test]
fn an_untyped_string_in_date_time_form_is_a_string_held_as_written() {
    let state = committed(&document(&json!({ex("at"): [property(&json!([{"@value": "2024-01-01T00:00:00.250000Z"}]))]})));
    let rows = round_trip(&state);
    assert!(matches!(only_value(&rows), StoredValue::String { text, annotation: LiteralAnnotation::Plain } if text == "2024-01-01T00:00:00.250000Z"));
    pretty_assertions::assert_eq!(only_value(&rows).kind(), ValueKind::String);
}

/// Annex C.6: `valueType` declares the datatype of the plain value beside it,
/// which is held as its instant and read back plain in its served spelling.
#[test]
fn a_value_type_date_time_reads_back_plain_in_its_served_spelling() {
    let state = committed(&document(&json!({ex("at"): [with(
        property(&json!([{"@value": "2024-01-01T00:00:00.000Z"}])),
        &json!({ngsi("hasValueType"): [{"@id": ngsi("DateTime")}]})
    )]})));
    let rows = round_trip(&state);
    assert!(matches!(only_value(&rows), StoredValue::DateTime { datatype: None, .. }));
    let FamilyValue::Property { values, .. } = held(state.attributes(), &ex("at"), None).value() else {
        panic!("a Property")
    };
    pretty_assertions::assert_eq!(
        values.as_slice(),
        [ExpandedValue::Literal(Literal::plain(LiteralValue::String("2024-01-01T00:00:00Z".into())))]
    );
}

/// Clause 4.6.3 admits a comma for the decimal point in requests; the broker
/// writes the point.
#[test]
fn an_expires_at_with_a_comma_fraction_comes_back_with_a_decimal_point() {
    let state = committed(&document(
        &json!({ex("at"): [with(property(&json!([{"@value": 1}])), &json!({ngsi("expiresAt"): date_time("2024-01-01T00:00:00,5Z")}))]}),
    ));
    round_trip(&state);
    pretty_assertions::assert_eq!(
        held(state.attributes(), &ex("at"), None)
            .expires_at()
            .map(|expires_at| expires_at.to_string())
            .as_deref(),
        Some("2024-01-01T00:00:00.500Z")
    );
}

#[test]
fn a_typed_date_is_held_under_the_date_kind() {
    let state = committed(&document(
        &json!({ex("on"): [property(&json!([{"@type": ngsi("Date"), "@value": "2024-01-01"}]))]}),
    ));
    let rows = round_trip(&state);
    assert!(matches!(only_value(&rows), StoredValue::Date { date, datatype: Some(_) } if date.to_string() == "2024-01-01"));
    pretty_assertions::assert_eq!(only_value(&rows).kind(), ValueKind::Date);
}

/// A `Time` is held in its fixed-width storage encoding under the `Time`
/// kind and read back typed in its served spelling.
#[test]
fn a_typed_time_is_held_in_its_storage_encoding_and_read_back_served() {
    let state = committed(&document(
        &json!({ex("at"): [property(&json!([{"@type": ngsi("Time"), "@value": "12:00:00.5Z"}]))]}),
    ));
    let rows = round_trip(&state);
    assert!(matches!(only_value(&rows), StoredValue::Time { time, datatype: Some(_) } if time.to_string() == "12:00:00.500000Z"));
    pretty_assertions::assert_eq!(only_value(&rows).kind(), ValueKind::Time);
    let FamilyValue::Property { values, .. } = held(state.attributes(), &ex("at"), None).value() else {
        panic!("a Property")
    };
    let [ExpandedValue::Literal(literal)] = values.as_slice() else {
        panic!("one literal")
    };
    pretty_assertions::assert_eq!(literal.value(), &LiteralValue::String("12:00:00.500Z".into()));
}

#[test]
fn a_custom_datatype_keeps_its_lexical_form() {
    let state = committed(&document(
        &json!({ex("d"): [property(&json!([{"@type": "http://www.w3.org/2001/XMLSchema#decimal", "@value": "1.10"}]))]}),
    ));
    let rows = round_trip(&state);
    assert!(matches!(only_value(&rows), StoredValue::String { text, annotation: LiteralAnnotation::Datatype(_) } if text == "1.10"));
}

#[test]
fn a_typed_number_keeps_its_datatype_beside_the_number() {
    let state = committed(&document(
        &json!({ex("n"): [property(&json!([{"@type": "http://www.w3.org/2001/XMLSchema#int", "@value": 5}]))]}),
    ));
    let rows = round_trip(&state);
    assert!(matches!(only_value(&rows), StoredValue::Number { datatype: Some(_), .. }));
}

#[test]
fn a_language_tagged_literal_keeps_its_tag() {
    let state = committed(&document(
        &json!({ex("greeting"): [property(&json!([{"@value": "Hallo", "@language": "de"}]))]}),
    ));
    let rows = round_trip(&state);
    assert!(matches!(only_value(&rows), StoredValue::String { annotation: LiteralAnnotation::Language(language), .. } if language.as_str() == "de"));
}

#[test]
fn a_literal_with_a_direction_is_one_value_object() {
    let state = committed(&document(
        &json!({ex("greeting"): [property(&json!([{"@value": "x", "@language": "ar", "@direction": "rtl"}]))]}),
    ));
    let rows = round_trip(&state);
    pretty_assertions::assert_eq!(only_value(&rows).kind(), ValueKind::Object);
}

#[test]
fn a_multi_valued_property_is_one_value_object() {
    let state = committed(&document(
        &json!({ex("mixed"): [property(&json!([{"@value": 1}, {"@value": "a"}, {"@value": true}]))]}),
    ));
    let rows = round_trip(&state);
    pretty_assertions::assert_eq!(ValueKind::code(only_value(&rows).kind()), 5);
    pretty_assertions::assert_eq!(object_text(only_value(&rows)), r#"[1,"a",true]"#);
}

#[test]
fn an_empty_property_value_round_trips() {
    let state = committed(&document(&json!({ex("none"): [property(&json!([]))]})));
    let rows = round_trip(&state);
    pretty_assertions::assert_eq!(object_text(only_value(&rows)), "[]");
}

#[test]
fn a_structured_value_is_one_value_object() {
    let state = committed(&document(&json!({ex("address"): [property(&json!([{
        ex("street"): [{"@value": "Main"}],
        ex("inner"): [{ex("deeper"): [{ex("deepest"): [{"@value": 3}]}]}]
    }]))]})));
    let rows = round_trip(&state);
    let FamilyValue::Property { values, .. } = held(state.attributes(), &ex("address"), None).value() else {
        panic!("a Property")
    };
    let history = StructuredValue::Values(values.iter().map(ExpandedValue::to_fragment).collect())
        .to_stored_json()
        .expect("the codec writes the value");
    pretty_assertions::assert_eq!(object_text(only_value(&rows)), history.get());
}

#[test]
fn a_json_property_is_opaque() {
    let state = committed(&document(
        &json!({ex("config"): [{"@type": family("JsonProperty"), ngsi("hasJSON"): [{"@type": "@json", "@value": {"a": [1, {"b": null}]}}]}]}),
    ));
    let rows = round_trip(&state);
    pretty_assertions::assert_eq!(object_text(only_value(&rows)), r#"{"a":[1,{"b":null}]}"#);
}

#[test]
fn a_language_map_round_trips() {
    let state = committed(&document(
        &json!({ex("name"): [{"@type": family("LanguageProperty"), ngsi("hasLanguageMap"): [{"@value": "x", "@language": "en"}, {"@value": "y", "@language": "de"}]}]}),
    ));
    round_trip(&state);
}

#[test]
fn a_single_vocab_term_stays_a_string() {
    let state = committed(&document(
        &json!({ex("category"): [{"@type": family("VocabProperty"), ngsi("hasVocab"): [{"@id": ex("office")}]}]}),
    ));
    let rows = round_trip(&state);
    assert!(matches!(only_value(&rows), StoredValue::String { text, annotation: LiteralAnnotation::Plain } if *text == ex("office")));
}

#[test]
fn a_single_object_relationship_stays_inline() {
    let state = committed(&document(
        &json!({ex("owner"): [{"@type": family("Relationship"), ngsi("hasObject"): [{"@id": "urn:a"}]}]}),
    ));
    let rows = round_trip(&state);
    assert!(matches!(only_value(&rows), StoredValue::Uri(uri) if uri.as_str() == "urn:a"));
    pretty_assertions::assert_eq!(row_column(only_value(&rows)).to_string(), "value_uri");
}

#[test]
fn a_multi_object_relationship_keeps_object_order() {
    let state = committed(&document(
        &json!({ex("owner"): [{"@type": family("Relationship"), ngsi("hasObject"): [{"@id": "urn:b"}, {"@id": "urn:a"}, {"@id": "urn:c"}]}]}),
    ));
    let rows = round_trip(&state);
    pretty_assertions::assert_eq!(object_text(only_value(&rows)), r#"["urn:b","urn:a","urn:c"]"#);
}

#[test]
fn a_multi_object_sub_attribute_relationship_keeps_object_order() {
    let state = committed(&document(&json!({ex("holder"): [with(property(&json!([{"@value": 1}])), &json!({
        ex("owner"): [{"@type": family("Relationship"), ngsi("hasObject"): [{"@id": "urn:b"}, {"@id": "urn:a"}]}]
    }))]})));
    let rows = round_trip(&state);
    let [element] = rows[0].sub_attributes.as_slice() else {
        panic!("one element")
    };
    pretty_assertions::assert_eq!(object_text(&element.members.value), r#"["urn:b","urn:a"]"#);
}

#[test]
fn a_list_relationship_keeps_duplicates() {
    let item = |target: &str| json!({ngsi("hasObject"): [{"@id": target}]});
    let state = committed(&document(
        &json!({ex("route"): [{"@type": family("ListRelationship"), ngsi("hasObjectList"): [{"@list": [item("urn:x"), item("urn:x"), item("urn:y")]}]}]}),
    ));
    let rows = round_trip(&state);
    pretty_assertions::assert_eq!(object_text(only_value(&rows)), r#"["urn:x","urn:x","urn:y"]"#);
}

#[test]
fn list_relationship_items_written_bare_come_back_wrapped() {
    let state = committed(&document(
        &json!({ex("route"): [{"@type": family("ListRelationship"), ngsi("hasObjectList"): [{"@list": [{"@id": "urn:x"}, {"@value": "urn:y"}]}]}]}),
    ));
    let rows = round_trip(&state);
    pretty_assertions::assert_eq!(object_text(only_value(&rows)), r#"["urn:x","urn:y"]"#);
}

#[test]
fn an_object_longer_than_the_index_bound_goes_to_the_long_column() {
    let target = format!("urn:ngsi-ld:Building:{}", "b".repeat(1025 - "urn:ngsi-ld:Building:".len()));
    let state = committed(&document(
        &json!({ex("owner"): [{"@type": family("Relationship"), ngsi("hasObject"): [{"@id": target}]}]}),
    ));
    let rows = round_trip(&state);
    pretty_assertions::assert_eq!(row_column(only_value(&rows)).to_string(), "value_long_uri");
}

#[test]
fn a_long_sub_attribute_string_stays_in_its_field() {
    let text = "s".repeat(5000);
    let state = committed(&document(
        &json!({ex("holder"): [with(property(&json!([{"@value": 1}])), &json!({ex("note"): [property(&json!([{"@value": text}]))]}))]}),
    ));
    let (keys, _) = dictionary(&state);
    let mut flattened = flatten(state.attributes(), &keys).expect("a committed state flattens");
    let element = flattened.remove(0).sub_attributes.remove(0);
    let record = SubAttributeRecord::try_from(element).expect("the element encodes");
    pretty_assertions::assert_eq!(record.value_string.as_deref().map(str::len), Some(5000));
    round_trip(&state);
}

#[test]
fn relationship_object_types_round_trip() {
    let relationship =
        json!({"@type": family("Relationship"), ngsi("hasObject"): [{"@id": "urn:a"}], ngsi("hasObjectType"): [{"@id": ex("A")}, {"@id": ex("B")}]});
    let state = committed(&document(
        &json!({ex("owner"): [with(relationship.clone(), &json!({ex("inner"): [relationship]}))]}),
    ));
    let rows = round_trip(&state);
    pretty_assertions::assert_eq!(rows[0].members.object_type_keys.len(), 2);
    pretty_assertions::assert_eq!(rows[0].sub_attributes[0].members.object_type_keys.len(), 2);
}

#[test]
fn a_geometry_keeps_its_bbox_and_foreign_members() {
    let geo = point(&json!([{"@value": 1}, {"@value": 2}]), &geometry_members());
    let state = committed(&document(&json!({ex("location"): [{"@type": family("GeoProperty"), ngsi("hasValue"): [geo]}]})));
    let rows = round_trip(&state);
    assert!(matches!(only_value(&rows), StoredValue::Geometry { members: Some(_), .. }));
}

#[test]
fn a_geo_sub_attribute_keeps_its_bbox_and_foreign_members() {
    let geo = point(&json!([{"@value": 1}, {"@value": 2}]), &geometry_members());
    let state = committed(&document(&json!({ex("holder"): [with(property(&json!([{"@value": 1}])), &json!({
        ex("location"): [{"@type": family("GeoProperty"), ngsi("hasValue"): [geo]}]
    }))]})));
    let rows = round_trip(&state);
    assert!(matches!(
        &rows[0].sub_attributes[0].members.value,
        StoredValue::Geometry { members: Some(_), .. }
    ));
}

#[test]
fn an_encoded_geometry_stays_a_string() {
    let encoded = r#"{"type":"Point","coordinates":[1,2]}"#;
    let state = committed(&document(
        &json!({ex("location"): [{"@type": family("GeoProperty"), ngsi("hasValue"): [{"@value": encoded}]}]}),
    ));
    let rows = round_trip(&state);
    assert!(matches!(only_value(&rows), StoredValue::String { text, .. } if text == encoded));
}

#[test]
fn integer_ordinates_round_trip() {
    let state = committed(&document(
        &json!({ex("location"): [{"@type": family("GeoProperty"), ngsi("hasValue"): [point(&json!([{"@value": 13}, {"@value": 52}]), &json!({}))]}]}),
    ));
    round_trip(&state);
    let rendered = String::from_utf8(state.to_vec().expect("the state encodes")).expect("UTF-8");
    assert!(rendered.contains(r#"[{"@value":13},{"@value":52}]"#), "{rendered}");
}

/// ETSI GS CIM 009 v1.9.1 clause 4.7.2 leaves position precision to the
/// implementation, so an ordinate is stored as its nearest double, while a
/// Property value keeps every digit it was written with.
#[test]
fn an_ordinate_is_stored_as_its_nearest_double_and_a_property_number_as_written() {
    let digits = "13.123456789012345678";
    let state = committed_text(&format!(
        r#"[{{"@id": "urn:ngsi-ld:Building:1", "@type": ["{EX}Building"],
            "{EX}location": [{{"@type": ["{NGSI}GeoProperty"], "{NGSI}hasValue": [{{"@type": ["{GEOJSON}Point"], "{GEOJSON}coordinates": [{{"@list": [{{"@value": {digits}}}, {{"@value": 52.5}}]}}]}}]}}],
            "{EX}girth": [{{"@type": ["{NGSI}Property"], "{NGSI}hasValue": [{{"@value": {digits}}}]}}]}}]"#
    ));
    round_trip(&state);
    let rendered = String::from_utf8(state.to_vec().expect("the state encodes")).expect("UTF-8");
    assert!(rendered.contains(r#"[{"@value":13.123456789012346},{"@value":52.5}]"#), "{rendered}");
    assert!(rendered.contains(r#"{"@value":13.123456789012345678}"#), "{rendered}");
}

#[test]
fn dataset_instances_keep_their_order() {
    let instance = |value: i64, dataset_id: Option<&str>| match dataset_id {
        Some(dataset_id) => with(property(&json!([{"@value": value}])), &json!({ngsi("datasetId"): [{"@id": dataset_id}]})),
        None => property(&json!([{"@value": value}])),
    };
    let state = committed(&document(
        &json!({ex("speed"): [instance(1, None), instance(2, Some("urn:ds:2")), instance(3, Some("urn:ds:1"))]}),
    ));
    let rows = round_trip(&state);
    pretty_assertions::assert_eq!(rows.iter().map(|row| row.ordinal).collect::<Vec<_>>(), vec![0, 1, 2]);
}

#[test]
fn sub_attribute_dataset_instances_keep_their_order() {
    let instance = |value: i64, dataset_id: Option<&str>| match dataset_id {
        Some(dataset_id) => with(property(&json!([{"@value": value}])), &json!({ngsi("datasetId"): [{"@id": dataset_id}]})),
        None => property(&json!([{"@value": value}])),
    };
    let state = committed(&document(&json!({ex("holder"): [with(property(&json!([{"@value": 0}])), &json!({
        ex("accuracy"): [instance(1, Some("urn:ds:2")), instance(2, None), instance(3, Some("urn:ds:1"))]
    }))]})));
    let rows = round_trip(&state);
    pretty_assertions::assert_eq!(
        rows[0]
            .sub_attributes
            .iter()
            .map(|element| element.members.dataset_id.as_ref().map(|dataset_id| dataset_id.as_str().to_owned()))
            .collect::<Vec<_>>(),
        vec![Some("urn:ds:2".to_owned()), None, Some("urn:ds:1".to_owned())]
    );
}

#[test]
fn attribute_order_is_kept() {
    // Written as text: a JSON object built in code orders its keys.
    let attribute = |name: &str, value: i64| format!(r#""{EX}{name}": [{{"@type": ["{NGSI}Property"], "{NGSI}hasValue": [{{"@value": {value}}}]}}]"#);
    let state = committed_text(&format!(
        r#"[{{"@id": "urn:ngsi-ld:Building:1", "@type": ["{EX}Building"], {}, {}, {}}}]"#,
        attribute("z", 1),
        attribute("a", 2),
        attribute("m", 3)
    ));
    let rows = round_trip(&state);
    pretty_assertions::assert_eq!(rows.iter().map(|row| row.ordinal).collect::<Vec<_>>(), vec![0, 1, 2]);
    pretty_assertions::assert_eq!(
        state.attributes().names().map(AttributeName::as_str).collect::<Vec<_>>(),
        vec![ex("z"), ex("a"), ex("m")]
    );
}

#[test]
fn elements_come_in_pre_order() {
    let state = committed(&document(&json!({ex("a"): [with(property(&json!([{"@value": 0}])), &json!({
        ex("b"): [with(property(&json!([{"@value": 1}])), &json!({ex("c"): [property(&json!([{"@value": 2}]))]}))],
        ex("d"): [property(&json!([{"@value": 3}]))]
    }))]})));
    let (keys, _) = dictionary(&state);
    let rows = round_trip(&state);
    let key = |name: &str| keys.attribute(&name.parse().expect("a name")).expect("a key");
    pretty_assertions::assert_eq!(
        rows[0]
            .sub_attributes
            .iter()
            .map(|element| (element.members.attribute_name_key, element.parent))
            .collect::<Vec<_>>(),
        vec![(key(&ex("b")), 0), (key(&ex("c")), 1), (key(&ex("d")), 0)]
    );
}

#[test]
fn nesting_at_the_configured_maximum_round_trips() {
    let mut chain = property(&json!([{"@value": 31}]));
    for depth in (1..31).rev() {
        chain = with(property(&json!([{"@value": depth}])), &json!({ex(&format!("level{}", depth + 1)): [chain]}));
    }
    let state = committed(&document(
        &json!({ex("level0"): [with(property(&json!([{"@value": 0}])), &json!({ex("level1"): [chain]}))]}),
    ));
    let rows = round_trip(&state);
    pretty_assertions::assert_eq!(
        rows[0].sub_attributes.iter().map(|element| element.parent).collect::<Vec<_>>(),
        (0..31).collect::<Vec<i32>>()
    );
}

/// 32,768 dataset instances of one sub-attribute put the child of the last
/// one at a position no `smallint` holds; the instance still round-trips.
#[test]
fn a_parent_past_the_smallint_range_round_trips() {
    const SIBLINGS: i32 = 32_768;
    let mut siblings: Vec<Value> = (1..SIBLINGS)
        .map(|n| {
            with(
                property(&json!([{"@value": n}])),
                &json!({ngsi("datasetId"): [{"@id": format!("urn:ngsi-ld:Dataset:{n}")}]}),
            )
        })
        .collect();
    siblings.push(with(
        property(&json!([{"@value": SIBLINGS}])),
        &json!({
            ngsi("datasetId"): [{"@id": format!("urn:ngsi-ld:Dataset:{SIBLINGS}")}],
            ex("c"): [property(&json!([{"@value": 0}]))]
        }),
    ));
    let state = committed(&document(
        &json!({ex("a"): [with(property(&json!([{"@value": 0}])), &json!({ex("b"): siblings}))]}),
    ));
    let rows = round_trip(&state);
    pretty_assertions::assert_eq!(rows[0].sub_attributes.last().map(|element| element.parent), Some(SIBLINGS));
}

#[test]
fn unit_code_value_type_and_times_round_trip() {
    let members = json!({
        ngsi("unitCode"): [{"@value": "KMH"}],
        ngsi("hasValueType"): [{"@id": ex("Speed")}],
        ngsi("observedAt"): date_time("2026-01-01T00:00:00.250Z"),
        ngsi("expiresAt"): date_time("2027-01-01T00:00:00Z")
    });
    let inner = with(property(&json!([{"@value": 1}])), &members);
    let mut state = committed(&document(
        &json!({ex("speed"): [with(with(property(&json!([{"@value": 2}])), &members), &json!({ex("accuracy"): [inner]}))]}),
    ));
    let accuracy = state
        .attributes_mut()
        .get_mut(&ex("speed"))
        .and_then(|speed| speed.get_mut(None))
        .and_then(|speed| speed.sub_attributes_mut().get_mut(&ex("accuracy")))
        .and_then(|accuracy| accuracy.get_mut(None))
        .expect("the sub-attribute");
    accuracy.set_created_at(Some(now()));
    accuracy.set_modified_at(Some(now()));
    let rows = round_trip(&state);
    let element = &rows[0].sub_attributes[0];
    assert!(element.members.unit_code.is_some() && element.members.value_type_key.is_some());
    assert!(element.members.observed_at.is_some() && element.members.expires_at.is_some());
    assert!(element.created_at.is_some() && element.modified_at.is_some());
}

#[test]
fn an_entity_without_attributes_is_no_rows() {
    let state = committed(&document(&json!({})));
    pretty_assertions::assert_eq!(round_trip(&state).len(), 0);
}

#[test]
fn rows_in_any_order_assemble_to_the_same_state() {
    let state = committed(&document(&every_family(&nested_every_family())));
    let (keys, names) = dictionary(&state);
    let mut shuffled = rows(&state, &keys);
    shuffled.reverse();
    shuffled.swap(1, 4);
    pretty_assertions::assert_eq!(&assemble(shuffled, &names).expect("the rows assemble"), state.attributes());
}

/// The rows of an Entity whose one attribute has three sub-attributes, `a`,
/// `b` and `c`, under the instance.
fn three_elements() -> (Vec<CurrentAttributeRow>, KeyNames) {
    let state = committed(&document(&json!({ex("holder"): [with(property(&json!([{"@value": 0}])), &json!({
        ex("a"): [property(&json!([{"@value": 1}]))],
        ex("b"): [property(&json!([{"@value": 2}]))],
        ex("c"): [property(&json!([{"@value": 3}]))]
    }))]})));
    let (keys, names) = dictionary(&state);
    (rows(&state, &keys), names)
}

#[test]
fn a_parent_after_its_child_is_an_assembly_error() {
    let (mut rows, names) = three_elements();
    rows[0].sub_attributes[0].parent = 2;
    assert!(matches!(
        assemble(rows, &names),
        Err(AssemblyError::ParentOutOfOrder { position: 1, parent: 2 })
    ));
}

#[test]
fn a_parent_outside_the_array_is_an_assembly_error() {
    let (mut rows, names) = three_elements();
    rows[0].sub_attributes[2].parent = 99;
    assert!(matches!(
        assemble(rows, &names),
        Err(AssemblyError::ParentOutsideArray { position: 3, parent: 99 })
    ));
}

#[test]
fn two_siblings_with_one_grain_is_an_assembly_error() {
    let (mut rows, names) = three_elements();
    rows[0].sub_attributes[1].members.attribute_name_key = rows[0].sub_attributes[0].members.attribute_name_key;
    assert!(matches!(assemble(rows, &names), Err(AssemblyError::RepeatedSubAttribute(_))));
}

#[test]
fn two_rows_with_one_grain_is_an_assembly_error() {
    let state = committed(&document(
        &json!({ex("a"): [property(&json!([{"@value": 1}]))], ex("b"): [property(&json!([{"@value": 2}]))]}),
    ));
    let (keys, names) = dictionary(&state);
    let mut rows = rows(&state, &keys);
    rows[1].members.attribute_name_key = rows[0].members.attribute_name_key;
    assert!(matches!(assemble(rows, &names), Err(AssemblyError::RepeatedInstance(_))));
}

#[test]
fn a_value_of_another_family_is_an_assembly_error() {
    let state = committed(&document(&json!({ex("a"): [property(&json!([{"@value": 1}]))]})));
    let (keys, names) = dictionary(&state);
    let mut rows = rows(&state, &keys);
    rows[0].members.attribute_kind = AttributeKind::Relationship;
    assert!(matches!(assemble(rows, &names), Err(AssemblyError::ValueOfAnotherFamily { .. })));
}

#[test]
fn a_name_without_a_key_is_a_flatten_error() {
    let state = committed(&document(&json!({ex("a"): [property(&json!([{"@value": 1}]))]})));
    assert!(matches!(
        flatten(state.attributes(), &NameKeys::default()),
        Err(FlattenError::UnresolvedAttribute(_))
    ));
}

#[test]
fn an_unstamped_instance_is_a_flatten_error() {
    let state = EntityState::from_slice(document(&json!({ex("a"): [property(&json!([{"@value": 1}]))]})).to_string().as_bytes()).expect("a stored entity");
    let (keys, _) = dictionary(&state);
    assert!(matches!(flatten(state.attributes(), &keys), Err(FlattenError::Unstamped(_))));
}

#[test]
fn an_uncommitted_datetime_spelling_is_a_flatten_error() {
    let mut state = EntityState::from_slice(
        document(&json!({ex("at"): [property(&json!([{"@type": ngsi("DateTime"), "@value": "2024-01-01T00:00:00.5Z"}]))]}))
            .to_string()
            .as_bytes(),
    )
    .expect("a stored entity");
    state.stamp_attribute_instants(None, now());
    let (keys, _) = dictionary(&state);
    assert!(matches!(flatten(state.attributes(), &keys), Err(FlattenError::UncommittedValue(_))));
}

/// Table 5.2.5-2 makes `instanceId` output-only, so current state holds none
/// and a document carrying one is held as if it carried none.
#[test]
fn an_instance_id_is_not_held() {
    let identified = committed(&document(
        &json!({ex("a"): [with(property(&json!([{"@value": 1}])), &json!({ngsi("instanceId"): [{"@id": "urn:ngsi-ld:Instance:1"}]}))]}),
    ));
    pretty_assertions::assert_eq!(identified, committed(&document(&json!({ex("a"): [property(&json!([{"@value": 1}]))]}))));
}
