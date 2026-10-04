//! The multi-instance `speed` entity the write tests operate on, the reader
//! that flattens it back to the values it holds, and the Property instances
//! and written instances those tests build their entities and fragments from.

use crate::{
    attribute_instances::AttributeInstances,
    attributes::Attributes,
    date_time_text::DateTimeText,
    document::{
        EntityDocument,
        attribute_member::AttributeMember,
        instance_document::InstanceDocument,
        slot::Slot,
        value_carrier::ValueCarrier,
        written_value::{ValueSlot, WrittenValue},
    },
    entity_state::EntityState,
    entity_types::EntityTypes,
    expanded_value::ExpandedValue,
    expanded_value_name::ExpandedValueName,
    family_value::FamilyValue,
    instance::Instance,
    literal::{Literal, LiteralValue},
    value_mode::{Fragment, ValueMode},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier};
use andromeda_ngsi_ld_number::JsonNumber;
use indexmap::IndexMap;

/// The attribute every speed fixture writes.
pub(crate) const SPEED: &str = "https://example.com/speed";

/// An expanded attribute name.
pub(crate) fn name(raw: &str) -> AttributeName {
    raw.parse().expect("a valid attribute name")
}

/// The expanded name of a member of a structured value.
pub(crate) fn member_name(raw: &str) -> ExpandedValueName {
    raw.parse().expect("a valid member name")
}

/// A `datasetId`.
pub(crate) fn dataset(raw: &str) -> Identifier {
    raw.parse().expect("a valid dataset identifier")
}

/// A `DateTime` text.
pub(crate) fn date_time(raw: &str) -> DateTimeText {
    raw.parse().expect("an RFC 3339 date-time")
}

/// The value object of an integer.
pub(crate) fn number<M: ValueMode>(value: i64) -> ExpandedValue<M> {
    ExpandedValue::Literal(Literal::plain(LiteralValue::Number(JsonNumber::from(value))))
}

/// The value object of a string.
pub(crate) fn text<M: ValueMode>(value: &str) -> ExpandedValue<M> {
    ExpandedValue::Literal(Literal::plain(LiteralValue::String(value.into())))
}

/// A stored Property instance holding `value`.
pub(crate) fn property(value: ExpandedValue) -> Instance {
    Instance::builder()
        .value(FamilyValue::Property {
            values: vec![value],
            unit_code: None,
            value_type: None,
        })
        .build()
}

/// A written Property instance holding `value`, optionally naming a
/// `datasetId`.
pub(crate) fn property_document(value: ExpandedValue<Fragment>, dataset_id: Option<&str>) -> InstanceDocument {
    InstanceDocument::builder()
        .family(Some(AttributeType::Property))
        .dataset_id(dataset_id.map(dataset))
        .value(ValueSlot::Set(WrittenValue::Value(vec![value])))
        .build()
}

/// A written Property instance that is its family's deletion encoding,
/// optionally naming a `datasetId`.
pub(crate) fn property_deletion(dataset_id: Option<&str>) -> InstanceDocument {
    InstanceDocument::builder()
        .family(Some(AttributeType::Property))
        .dataset_id(dataset_id.map(dataset))
        .value(ValueSlot::Remove(ValueCarrier::Value))
        .build()
}

/// A written Property instance holding `value` and naming `observedAt`.
pub(crate) fn observed_document(value: i64, dataset_id: Option<&str>, observed_at: &str) -> InstanceDocument {
    let mut document = property_document(number(value), dataset_id);
    document.set_observed_at(Some(Slot::Set(date_time(observed_at))));
    document
}

/// The instances of one attribute, by `datasetId`.
pub(crate) fn instances(instances: Vec<(Option<&str>, Instance)>) -> AttributeInstances {
    AttributeInstances::try_from_instances(instances.into_iter().map(|(dataset_id, instance)| (dataset_id.map(dataset), instance)))
        .expect("distinct datasetIds")
        .expect("at least one instance")
}

/// An entity holding the given attributes.
pub(crate) fn entity(attributes: Vec<(&str, AttributeInstances)>) -> EntityState {
    EntityState::from_parts(
        dataset("urn:ngsi-ld:Vehicle:1"),
        EntityTypes::one("https://example.com/Vehicle".parse().expect("a valid type")),
        None,
        None,
        attributes.into_iter().map(|(raw, instances)| (name(raw), instances)).collect::<Attributes>(),
    )
}

/// An entity with three `speed` instances: datasets `A`, `B`, and the
/// default.
pub(crate) fn with_three_instances() -> EntityState {
    entity(vec![(
        SPEED,
        instances(vec![
            (Some("urn:ngsi-ld:Dataset:A"), property(number(10))),
            (Some("urn:ngsi-ld:Dataset:B"), property(number(20))),
            (None, property(number(30))),
        ]),
    )])
}

/// A written entity fragment carrying the given attribute members.
pub(crate) fn fragment(attributes: Vec<(&str, AttributeMember)>) -> EntityDocument {
    EntityDocument::new(
        None,
        Vec::new(),
        None,
        None,
        attributes.into_iter().map(|(raw, member)| (name(raw), member)).collect::<IndexMap<_, _>>(),
    )
}

/// The integer values the instances of `attribute` hold, in stored order.
pub(crate) fn values(entity: &EntityState, attribute: &str) -> Vec<i64> {
    entity
        .attributes()
        .get(attribute)
        .map(|instances| instances.iter().filter_map(|(_, instance)| integer_of(instance)).collect())
        .unwrap_or_default()
}

/// The integer a Property instance holds.
pub(crate) fn integer_of(instance: &Instance) -> Option<i64> {
    match instance.value() {
        FamilyValue::Property { values, .. } => match values.as_slice() {
            [ExpandedValue::Literal(literal)] => match literal.value() {
                LiteralValue::Number(number) => number.to_json_value().as_i64(),
                LiteralValue::String(_) | LiteralValue::Boolean(_) => None,
            },
            [ExpandedValue::Json(_) | ExpandedValue::Node(_) | ExpandedValue::List(_)] | [] | [_, _, ..] => None,
        },
        FamilyValue::GeoProperty { .. }
        | FamilyValue::Relationship { .. }
        | FamilyValue::LanguageProperty { .. }
        | FamilyValue::VocabProperty { .. }
        | FamilyValue::JsonProperty { .. }
        | FamilyValue::ListProperty { .. }
        | FamilyValue::ListRelationship { .. } => None,
    }
}

/// The sub-attribute the sub-attribute write tests land.
pub(crate) const ACCURACY: &str = "https://example.org/accuracy";

/// The `(datasetId, integer)` of every instance of `attribute` among
/// `attributes`, in stored order.
pub(crate) fn instance_rows(attributes: &Attributes, attribute: &str) -> Vec<(Option<String>, Option<i64>)> {
    attributes
        .get(attribute)
        .map(|instances| {
            instances
                .iter()
                .map(|(dataset_id, instance)| (dataset_id.map(ToString::to_string), integer_of(instance)))
                .collect()
        })
        .unwrap_or_default()
}

/// One expected row of [`instance_rows`].
pub(crate) fn row(dataset_id: Option<&str>, value: i64) -> (Option<String>, Option<i64>) {
    (dataset_id.map(str::to_owned), Some(value))
}

/// The `datasetId`s of the instances of `attribute`, in stored order.
pub(crate) fn dataset_ids(entity: &EntityState, attribute: &str) -> Vec<Option<String>> {
    entity
        .attributes()
        .get(attribute)
        .map(|instances| instances.iter().map(|(dataset_id, _)| dataset_id.map(ToString::to_string)).collect())
        .unwrap_or_default()
}
