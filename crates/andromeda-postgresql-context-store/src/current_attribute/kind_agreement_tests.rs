//! The target kind the in-memory evaluator reads from an attribute instance's
//! own value is one of those the writer's stored kind for that value names.

use crate::current_attribute::{
    flatten::flatten,
    round_trip_tests::{committed, dictionary, document, ex, family, ngsi, property},
};
use andromeda_ngsi_ld_entity::{entity_state::EntityState, instance::Instance, queried::held_instance::HeldInstance};
use andromeda_ngsi_ld_query::filter::reading::target_kind::TargetKind;
use andromeda_ngsi_ld_query_evaluation::q::{family_target::family_target, target_value::ValueShape};
use andromeda_postgresql_query::q_predicate::stored_kind::stored_target_kinds;
use andromeda_postgresql_stored_values::value_kind::ValueKind;
use proptest::{prelude::*, strategy::BoxedStrategy};
use serde_json::{Value, json};

const XSD: &str = "http://www.w3.org/2001/XMLSchema#";

/// An expanded Property holding the one literal `{"@value": value}`, typed
/// `datatype` when given.
fn literal_property(value: &Value, datatype: Option<String>) -> Value {
    let literal = datatype.map_or_else(|| json!({"@value": value}), |datatype| json!({"@type": datatype, "@value": value}));
    property(&json!([literal]))
}

/// An expanded Relationship to `object`.
fn relationship(object: &str) -> Value {
    json!({"@type": family("Relationship"), ngsi("hasObject"): [{"@id": object}]})
}

/// The Entity state committing `instance` as the attribute `value`.
fn state_of(instance: &Value) -> EntityState {
    committed(&document(&json!({ex("value"): [instance]})))
}

/// The one instance the state holds, with no `datasetId`.
fn instance_of(state: &EntityState) -> &Instance {
    state
        .attributes()
        .get(ex("value").as_str())
        .and_then(|instances| instances.get(None))
        .expect("the instance")
}

/// The kind the writer stores the own value of `instance` under.
fn stored_kind(instance: &Value) -> ValueKind {
    let state = state_of(instance);
    let (keys, _) = dictionary(&state);
    let rows = flatten(state.attributes(), &keys).expect("a committed state flattens");
    let [row] = rows.as_slice() else {
        panic!("one row expected, got {}", rows.len())
    };
    row.members.value.kind()
}

/// The kind the evaluator reads from the own value of `instance`.
fn read_kind(instance: &Value) -> TargetKind {
    let state = state_of(instance);
    let target = family_target(HeldInstance::new(None, instance_of(&state))).expect("a target value");
    match target.value.shape(target.origin) {
        ValueShape::One(kind) => kind,
        ValueShape::Several(values) => panic!("one value expected, got {values:?}"),
    }
}

/// Assert that the stored kind of `instance` names the kind the evaluator
/// reads.
fn assert_agreement(label: &str, instance: &Value) {
    let (stored, read) = (stored_kind(instance), read_kind(instance));
    assert!(
        stored_target_kinds(stored).contains(&read),
        "{label}: the writer stores {stored:?}, naming {:?}, but the evaluator reads {read:?} from {instance}",
        stored_target_kinds(stored)
    );
}

/// Every scalar instance the writer stores in a typed field.
fn scalar_instance() -> BoxedStrategy<Value> {
    let integer = (-1_000_000_000_i64..1_000_000_000).prop_map(|number| literal_property(&json!(number), None));
    let decimal = (-100_000_i32..100_000, 0_u32..1000).prop_map(|(whole, fraction)| {
        let number: serde_json::Number = format!("{whole}.{fraction:03}").parse().expect("a JSON number");
        literal_property(&Value::Number(number), None)
    });
    let text = "[a-zA-Z ]{0,12}".prop_map(|text| literal_property(&json!(text), None));
    let boolean = any::<bool>().prop_map(|flag| literal_property(&json!(flag), None));
    let declared_number = (
        prop_oneof![
            Just("NaN".to_owned()),
            Just("INF".to_owned()),
            Just("-INF".to_owned()),
            (-100_000_i32..100_000, 0_u32..1000).prop_map(|(whole, fraction)| format!("{whole}.{fraction:03}")),
            (1_u32..100, -20_i32..20).prop_map(|(mantissa, exponent)| format!("{mantissa}e{exponent}")),
        ],
        prop_oneof![Just("double"), Just("float")],
    )
        .prop_map(move |(spelling, datatype)| literal_property(&json!(spelling), Some(format!("{XSD}{datatype}"))));
    let declared_boolean = prop_oneof![Just("true"), Just("false"), Just("1"), Just("0")]
        .prop_map(move |spelling| literal_property(&json!(spelling), Some(format!("{XSD}boolean"))));
    let date_time = (1970_u32..2100, 1_u32..13, 1_u32..29, 0_u32..24, 0_u32..60, 0_u32..60).prop_map(move |(year, month, day, hour, minute, second)| {
        literal_property(
            &json!(format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")),
            Some(ngsi("DateTime")),
        )
    });
    let date = (1970_u32..2100, 1_u32..13, 1_u32..29)
        .prop_map(|(year, month, day)| literal_property(&json!(format!("{year:04}-{month:02}-{day:02}")), Some(ngsi("Date"))));
    let time = (0_u32..24, 0_u32..60, 0_u32..60)
        .prop_map(|(hour, minute, second)| literal_property(&json!(format!("{hour:02}:{minute:02}:{second:02}Z")), Some(ngsi("Time"))));
    let object = (1_u32..100_000).prop_map(|number| relationship(&format!("urn:ngsi-ld:Person:{number}")));
    prop_oneof![
        integer,
        decimal,
        text,
        boolean,
        declared_number,
        declared_boolean,
        date_time,
        date,
        time,
        object
    ]
    .boxed()
}

proptest! {
    #[test]
    fn every_scalar_value_the_writer_stores_is_of_a_kind_its_stored_kind_names(instance in scalar_instance()) {
        assert_agreement("a generated instance", &instance);
    }
}

#[test]
fn each_scalar_kind_the_writer_stores_agrees_with_the_kind_the_evaluator_reads() {
    let declared = |value: Value, name: &str| literal_property(&value, Some(format!("{XSD}{name}")));
    let cases = [
        ("integer", ValueKind::Number, literal_property(&json!(42), None)),
        ("decimal", ValueKind::Number, literal_property(&json!(2.5), None)),
        ("plain text", ValueKind::String, literal_property(&json!("text"), None)),
        ("boolean", ValueKind::Boolean, literal_property(&json!(true), None)),
        ("xsd:double NaN", ValueKind::DeclaredNumber, declared(json!("NaN"), "double")),
        ("xsd:double INF", ValueKind::DeclaredNumber, declared(json!("INF"), "double")),
        ("xsd:float -INF", ValueKind::DeclaredNumber, declared(json!("-INF"), "float")),
        ("xsd:double finite", ValueKind::DeclaredNumber, declared(json!("1.5"), "double")),
        ("xsd:float finite", ValueKind::DeclaredNumber, declared(json!("2e3"), "float")),
        ("xsd:boolean", ValueKind::DeclaredBoolean, declared(json!("true"), "boolean")),
        (
            "DateTime",
            ValueKind::DateTime,
            literal_property(&json!("2024-01-01T00:00:00Z"), Some(ngsi("DateTime"))),
        ),
        ("Date", ValueKind::Date, literal_property(&json!("2024-01-01"), Some(ngsi("Date")))),
        ("Time", ValueKind::Time, literal_property(&json!("12:00:00Z"), Some(ngsi("Time")))),
        ("Relationship object", ValueKind::Uri, relationship("urn:ngsi-ld:Person:1")),
    ];
    for (label, expected, instance) in cases {
        pretty_assertions::assert_eq!(stored_kind(&instance), expected, "{label}");
        assert_agreement(label, &instance);
    }
}
