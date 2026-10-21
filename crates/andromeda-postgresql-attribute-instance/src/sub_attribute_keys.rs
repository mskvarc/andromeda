//! The clause 4.9 equality keys of one instance's sub-attribute tree, which
//! its `current_attributes` row stores in `sub_eq_keys`.
//!
//! Each element is keyed at its own name path, the top-level attribute first,
//! which it reaches through `parent`. An element holding one scalar has its
//! value's key; one holding an array has the key of every scalar member
//! ([`stored_array_members`]); one holding a geometry or a structured object
//! has none. A value declared as a time value is keyed under its kind, a
//! String under its own.

use crate::{
    stored_array_members::{ArrayMember, StoredArrayMemberError, stored_array_members},
    stored_value::StoredValue,
    sub_attribute::SubAttributeElement,
};
use andromeda_ngsi_ld_lexicon::time::value::TimeValue;
use andromeda_postgresql_encoding::array_literal::IntegerArrayLiteral;
use andromeda_postgresql_query::{
    attribute_name_key::AttributeNameKey,
    sub_attribute_key::{
        key::{KeyedValue, SubAttributeKey, TextKind},
        path_hash::PathHash,
    },
};
use thiserror::Error;

/// A sub-attribute tree whose keys cannot be computed.
#[derive(Debug, Error)]
pub enum SubAttributeKeyError {
    /// An element's array value does not read back.
    #[error(transparent)]
    Member(#[from] StoredArrayMemberError),
    /// An element names a parent that does not precede it.
    #[error("sub-attribute element {position} names parent {parent}, which does not precede it")]
    Parent {
        /// The 1-based array position of the element.
        position: usize,
        /// The parent it names.
        parent: i32,
    },
}

/// The equality keys of `elements`, the sub-attribute tree of an instance of
/// `attribute`, as the array literal `sub_eq_keys` is staged in; `None` when
/// no element holds a keyed value.
///
/// # Errors
///
/// Returns [`SubAttributeKeyError`] when an element names a parent that does
/// not precede it, or its array value does not read back.
pub fn sub_attribute_keys(attribute: AttributeNameKey, elements: &[SubAttributeElement]) -> Result<Option<IntegerArrayLiteral>, SubAttributeKeyError> {
    let root = PathHash::ROOT.child(attribute);
    let mut paths = Vec::with_capacity(elements.len());
    let mut keys = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        let parent = match element.parent {
            0 => Some(root),
            parent => usize::try_from(parent)
                .ok()
                .and_then(|position| position.checked_sub(1))
                .and_then(|parent_index| paths.get(parent_index).copied()),
        }
        .ok_or(SubAttributeKeyError::Parent {
            position: index + 1,
            parent: element.parent,
        })?;
        let path = parent.child(element.members.attribute_name_key);
        paths.push(path);
        push_value_keys(path, &element.members.value, &mut keys)?;
    }
    Ok((!keys.is_empty()).then(|| IntegerArrayLiteral::of(keys.into_iter().map(SubAttributeKey::get))))
}

/// Push the keys `value` has at `path` onto `keys`.
fn push_value_keys(path: PathHash, value: &StoredValue, keys: &mut Vec<SubAttributeKey>) -> Result<(), StoredArrayMemberError> {
    let key = |value: KeyedValue<'_>| SubAttributeKey::of(path, value);
    match value {
        StoredValue::Number { number, .. } => keys.push(key(KeyedValue::Number(number.decimal()))),
        StoredValue::String { text, .. } => keys.push(key(KeyedValue::Text(text, TextKind::String))),
        StoredValue::Boolean { flag, .. } => keys.push(key(KeyedValue::Boolean(*flag))),
        StoredValue::DateTime { instant, .. } => keys.push(key(KeyedValue::Instant(*instant))),
        StoredValue::Date { date, .. } => keys.push(key(KeyedValue::Text(&date.to_string(), TextKind::Date))),
        StoredValue::Time { time, .. } => keys.push(key(KeyedValue::Text(&time.to_string(), TextKind::Time))),
        StoredValue::Uri(uri) => keys.push(key(KeyedValue::Text(uri.as_str(), TextKind::Uri))),
        StoredValue::Object(json) => {
            for member in stored_array_members(json)? {
                match member {
                    ArrayMember::Number(number) => keys.push(key(KeyedValue::Number(number.decimal()))),
                    ArrayMember::Text(text) => keys.push(key(KeyedValue::Text(&text, TextKind::String))),
                    ArrayMember::Boolean(flag) => keys.push(key(KeyedValue::Boolean(flag))),
                    ArrayMember::Time(TimeValue::DateTime(instant)) => keys.push(key(KeyedValue::Instant(instant.get()))),
                    ArrayMember::Time(TimeValue::Date(date)) => keys.push(key(KeyedValue::Text(&date.to_string(), TextKind::Date))),
                    ArrayMember::Time(TimeValue::Time(time)) => keys.push(key(KeyedValue::Text(&time.to_string(), TextKind::Time))),
                }
            }
        }
        StoredValue::Geometry { .. } => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        stored_json::StoredJson,
        stored_members::InstanceMembers,
        stored_value::{LiteralAnnotation, StoredValue},
        sub_attribute::SubAttributeElement,
        sub_attribute_keys::{SubAttributeKeyError, sub_attribute_keys},
    };
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_postgresql_codes::attribute_kind::AttributeKind;
    use andromeda_postgresql_query::{
        attribute_name_key::AttributeNameKey,
        sub_attribute_key::{
            key::{KeyedValue, SubAttributeKey, TextKind},
            path_hash::PathHash,
        },
    };
    use bigdecimal::BigDecimal;
    use chrono::{DateTime, TimeZone, Utc};
    use pretty_assertions::{assert_eq, assert_ne};
    use serde_json::{json, value::RawValue};

    /// The `attribute_names` key of the top-level attribute.
    const ATTRIBUTE: i32 = 2;

    fn element(parent: i32, name: i32, value: StoredValue) -> SubAttributeElement {
        SubAttributeElement {
            parent,
            members: InstanceMembers {
                attribute_name_key: AttributeNameKey::from(name),
                dataset_id: None,
                attribute_kind: AttributeKind::Property,
                value,
                value_type_key: None,
                object_type_keys: Vec::new(),
                unit_code: None,
                observed_at: None,
                expires_at: None,
                proof_seals: None,
            },
            created_at: None,
            modified_at: None,
        }
    }

    fn number(text: &str) -> StoredValue {
        StoredValue::Number {
            number: text.parse::<JsonNumber>().expect("a number"),
            datatype: None,
        }
    }

    fn string(text: &str) -> StoredValue {
        StoredValue::String {
            text: text.to_owned(),
            annotation: LiteralAnnotation::Plain,
        }
    }

    fn object(text: &str) -> StoredValue {
        StoredValue::Object(StoredJson::new(RawValue::from_string(text.to_owned()).expect("JSON")))
    }

    fn midnight() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).single().expect("a valid instant")
    }

    /// The keys of `elements`, read back from their array literal.
    fn keys(elements: &[SubAttributeElement]) -> Option<Vec<i64>> {
        sub_attribute_keys(AttributeNameKey::from(ATTRIBUTE), elements)
            .expect("the keys compute")
            .map(|literal| {
                literal
                    .as_str()
                    .trim_matches(['{', '}'])
                    .split(',')
                    .map(|key| key.parse().expect("a bigint"))
                    .collect()
            })
    }

    /// The key `value` has at the name path `names` under the attribute.
    fn key_at(names: &[i32], value: KeyedValue<'_>) -> i64 {
        let path = PathHash::of(&[ATTRIBUTE].iter().chain(names).copied().map(AttributeNameKey::from).collect::<Vec<_>>());
        SubAttributeKey::of(path, value).get()
    }

    #[test]
    fn each_element_is_keyed_at_its_own_path() {
        let one = BigDecimal::from(1);
        let keys = keys(&[element(0, 5, number("1")), element(1, 9, number("1"))]).expect("keyed");
        assert_eq!(keys, vec![key_at(&[5], KeyedValue::Number(&one)), key_at(&[5, 9], KeyedValue::Number(&one))]);
        assert_ne!(keys[0], keys[1]);
    }

    #[test]
    fn a_number_element_carries_the_key_of_its_value() {
        let one = BigDecimal::from(1);
        assert_eq!(keys(&[element(0, 5, number("1.00"))]), Some(vec![key_at(&[5], KeyedValue::Number(&one))]));
    }

    /// Annex C.6: a string declared as no time value is a String.
    #[test]
    fn a_plain_string_in_a_time_value_form_carries_its_string_key_alone() {
        assert_eq!(
            keys(&[element(0, 5, string("12:00:00.000Z"))]),
            Some(vec![key_at(&[5], KeyedValue::Text("12:00:00.000Z", TextKind::String))])
        );
    }

    #[test]
    fn a_time_element_carries_the_time_key_of_its_fixed_width_encoding() {
        let value = StoredValue::Time {
            time: "12:00:00.5Z".parse().expect("a Time"),
            datatype: None,
        };
        assert_eq!(
            keys(&[element(0, 5, value)]),
            Some(vec![key_at(&[5], KeyedValue::Text("12:00:00.500000Z", TextKind::Time))])
        );
    }

    #[test]
    fn a_date_element_carries_the_date_key_of_its_text() {
        let value = StoredValue::Date {
            date: "2024-01-01".parse().expect("a Date"),
            datatype: None,
        };
        assert_eq!(
            keys(&[element(0, 5, value)]),
            Some(vec![key_at(&[5], KeyedValue::Text("2024-01-01", TextKind::Date))])
        );
    }

    #[test]
    fn a_date_time_element_carries_its_instant_key() {
        let value = StoredValue::DateTime {
            instant: midnight(),
            datatype: None,
        };
        assert_eq!(keys(&[element(0, 5, value)]), Some(vec![key_at(&[5], KeyedValue::Instant(midnight()))]));
    }

    #[test]
    fn a_typed_date_time_member_of_an_array_carries_its_instant_key_and_a_plain_one_its_text() {
        let keys = keys(&[element(
            0,
            5,
            object(r#"[{"@value":"2024-01-01T00:00:00Z","@type":"https://uri.etsi.org/ngsi-ld/DateTime"},"2024-01-01T00:00:00Z"]"#),
        )])
        .expect("keyed");
        assert_eq!(
            keys,
            vec![
                key_at(&[5], KeyedValue::Instant(midnight())),
                key_at(&[5], KeyedValue::Text("2024-01-01T00:00:00Z", TextKind::String)),
            ]
        );
    }

    #[test]
    fn a_language_map_carries_the_key_of_each_text() {
        let keys = keys(&[element(
            0,
            5,
            object(r#"[{"@value":"Hallo","@language":"de"},{"@value":"Hello","@language":"en"}]"#),
        )])
        .expect("keyed");
        assert_eq!(
            keys,
            vec![
                key_at(&[5], KeyedValue::Text("Hallo", TextKind::String)),
                key_at(&[5], KeyedValue::Text("Hello", TextKind::String)),
            ]
        );
    }

    #[test]
    fn a_relationship_object_is_keyed_as_a_uri() {
        let uri = StoredValue::Uri("urn:ngsi-ld:Device:1".parse().expect("a URI"));
        assert_eq!(
            keys(&[element(0, 5, uri)]),
            Some(vec![key_at(&[5], KeyedValue::Text("urn:ngsi-ld:Device:1", TextKind::Uri))])
        );
    }

    #[test]
    fn a_tree_of_geometries_and_structured_values_has_no_key() {
        let point = StoredValue::Geometry {
            form: serde_json::from_value::<Geometry>(json!({"type": "Point", "coordinates": [13.5, 52.25]}))
                .expect("a point")
                .into_parts()
                .0,
            members: None,
        };
        assert_eq!(
            keys(&[element(0, 5, point), element(0, 6, object(r#"[{"https://example.com/k":[{"@value":1}]}]"#))]),
            None
        );
    }

    #[test]
    fn an_instance_without_sub_attributes_has_no_key() {
        assert_eq!(keys(&[]), None);
    }

    #[test]
    fn an_element_whose_parent_does_not_precede_it_is_refused() {
        let refusal = sub_attribute_keys(AttributeNameKey::from(ATTRIBUTE), &[element(1, 5, number("1"))]);
        assert!(matches!(refusal, Err(SubAttributeKeyError::Parent { position: 1, parent: 1 })), "{refusal:?}");
    }
}
