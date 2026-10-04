//! Unfolding the simplified temporal representation of a Temporal Evolution
//! (clause 4.5.9) into the instances it stands for.
//!
//! An attribute object in that representation holds its family's plural member
//! (`values`, `objects`, `languageMaps`, `vocabs`, `jsons`, `valueLists`,
//! `objectLists`), whose every element is a `[value, instant]` pair. Clause
//! 4.5.9 has the object "only contain" that member; besides it the object
//! carries its `type`, which names the family, and the `datasetId` of the
//! instances it gathers, since a temporal representation gives each `datasetId`
//! its own object (clause 4.5.5). Anything else is refused.
//!
//! Each pair becomes one instance: the object's `type` and `datasetId`, the
//! family's value member holding the pair's value -- taken out of the
//! one-member object clause 4.5.9 wraps the value of a `LanguageProperty`, a
//! `JsonProperty` and a `VocabProperty` in -- and the pair's instant as
//! `observedAt` when that is the Temporal Property it was paired on. An
//! instance copies nothing but `type` and `datasetId`, so the unfold builds no
//! more than the pairs already hold, and the written document is held to the
//! entity budget after expansion like any other.

use crate::{error::TemporalError, plural_member_refusal::PluralMemberRefusal, temporal_validation_reason::TemporalValidationReason};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::{AttributeType, TemporalValueEncoding},
    time_prop::TimeProp,
};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde_json::{Map, Value};
use std::slice::from_ref;

/// The members an attribute object holding its plural member carries beside
/// it.
const MEMBERS_BESIDE_PAIRS: [&str; 2] = ["type", compact_properties::DATASET_ID];

/// Unfold every attribute object of `payload` that holds its family's plural
/// member into the instances its pairs stand for, each pair's instant kept as
/// `observedAt` when `timeproperty` is the one the pairs were made on.
///
/// # Errors
///
/// Returns [`TemporalError::Validation`] when such an object carries another
/// member, its plural member is not an array of pairs, a pair's value is not
/// in its family's form, or a pair's `observedAt` is not a string.
pub(crate) fn unfold_plural_members(payload: &mut Value, timeproperty: TimeProp) -> Result<(), TemporalError> {
    let Some(members) = payload.as_object_mut() else {
        return Ok(());
    };
    for (name, member) in members.iter_mut() {
        if !instances(member).iter().any(|instance| instance.as_object().and_then(plural_family).is_some()) {
            continue;
        }
        let written = match member.take() {
            Value::Array(instances) => instances,
            single @ (Value::Object(_) | Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => vec![single],
        };
        let mut unfolded = Vec::with_capacity(written.len());
        for instance in written {
            unfold_instance(name, instance, timeproperty, &mut unfolded)?;
        }
        *member = Value::Array(unfolded);
    }
    Ok(())
}

/// The instances an attribute member holds: one instance object, or an array
/// of them.
fn instances(member: &Value) -> &[Value] {
    match member {
        Value::Array(instances) => instances,
        Value::Object(_) => from_ref(member),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => &[],
    }
}

/// The family of an attribute object that holds its family's plural member.
fn plural_family(instance: &Map<String, Value>) -> Option<AttributeType> {
    let family = instance.get("type")?.as_str()?.parse::<AttributeType>().ok()?;
    instance.contains_key(family.temporal_values_key()).then_some(family)
}

/// Push onto `unfolded` the instances one instance of the attribute `name`
/// stands for: itself, or one per pair of its plural member.
fn unfold_instance(name: &str, instance: Value, timeproperty: TimeProp, unfolded: &mut Vec<Value>) -> Result<(), TemporalError> {
    let Value::Object(mut envelope) = instance else {
        unfolded.push(instance);
        return Ok(());
    };
    let Some(family) = plural_family(&envelope) else {
        unfolded.push(Value::Object(envelope));
        return Ok(());
    };
    let refused = |fault| refusal(name, family, fault);
    let Some(Value::Array(pairs)) = envelope.remove(family.temporal_values_key()) else {
        return Err(refused(PairFault::NotAnArray));
    };
    if let Some(member) = envelope.keys().find(|member| !MEMBERS_BESIDE_PAIRS.contains(&member.as_str())) {
        return Err(refused(PairFault::MemberBesidePairs(member)));
    }
    unfolded.reserve(pairs.len());
    for pair in pairs {
        let instance = pair_instance(&envelope, pair, family, timeproperty).map_err(refused)?;
        unfolded.push(Value::Object(instance));
    }
    Ok(())
}

/// The instance one `[value, instant]` pair of a `family` object carrying
/// `envelope` beside its plural member stands for.
fn pair_instance(envelope: &Map<String, Value>, pair: Value, family: AttributeType, timeproperty: TimeProp) -> Result<Map<String, Value>, PairFault<'static>> {
    let Value::Array(elements) = pair else {
        return Err(PairFault::NotAPair);
    };
    let [value, instant] = <[Value; 2]>::try_from(elements).map_err(|_unpaired| PairFault::NotAPair)?;
    let value = match family.temporal_value_encoding() {
        TemporalValueEncoding::Wrapped => unwrapped(value, family.compact_value_key())?,
        TemporalValueEncoding::Bare => value,
    };
    let mut instance = envelope.clone(); // clone: every pair carries the `type` and `datasetId` of the object it is written in
    instance.insert(family.compact_value_key().to_owned(), value);
    match timeproperty {
        TimeProp::Observed if instant.is_string() => {
            instance.insert(compact_properties::OBSERVED_AT.to_owned(), instant);
        }
        TimeProp::Observed => return Err(PairFault::InstantNotAString),
        // A system time is the broker's to set, never one an instance carries
        // into a write, so the instant positions nothing here.
        TimeProp::Created | TimeProp::Modified | TimeProp::Deleted => {}
    }
    Ok(instance)
}

/// The value a wrapped pair element carries: the element is an object
/// holding `value_member` and nothing else.
fn unwrapped(element: Value, value_member: &str) -> Result<Value, PairFault<'static>> {
    let Value::Object(mut wrapper) = element else {
        return Err(PairFault::UnwrappedValue);
    };
    match (wrapper.remove(value_member), wrapper.is_empty()) {
        (Some(value), true) => Ok(value),
        (Some(_) | None, false) | (None, true) => Err(PairFault::UnwrappedValue),
    }
}

/// What is wrong with an attribute object holding its plural member, before
/// the attribute is named.
#[derive(Clone, Copy, Debug)]
enum PairFault<'a> {
    /// It carries this member beside the pairs.
    MemberBesidePairs(&'a str),
    /// The plural member is not an array.
    NotAnArray,
    /// An element is not a two-element array.
    NotAPair,
    /// A wrapped family's value is not wrapped.
    UnwrappedValue,
    /// An `observedAt` is not a string.
    InstantNotAString,
}

/// The refusal of the attribute `name` of `family` for `fault`.
fn refusal(name: &str, family: AttributeType, fault: PairFault<'_>) -> TemporalError {
    let named = |raw: &str| {
        raw.parse::<AttributeName>()
            .map_err(|source| TemporalError::from(TemporalValidationReason::AttributeName { source }))
    };
    let attribute = match named(name) {
        Ok(attribute) => attribute,
        Err(refused) => return refused,
    };
    let refused = match fault {
        PairFault::MemberBesidePairs(member) => match named(member) {
            Ok(member) => PluralMemberRefusal::MemberBesidePairs { attribute, family, member },
            Err(refused) => return refused,
        },
        PairFault::NotAnArray => PluralMemberRefusal::NotAnArray { attribute, family },
        PairFault::NotAPair => PluralMemberRefusal::NotAPair { attribute, family },
        PairFault::UnwrappedValue => PluralMemberRefusal::UnwrappedValue { attribute, family },
        PairFault::InstantNotAString => PluralMemberRefusal::InstantNotAString { attribute, family },
    };
    TemporalError::from(TemporalValidationReason::from(refused))
}

#[cfg(test)]
mod tests {
    use crate::{
        error::TemporalError,
        plural_member_refusal::PluralMemberRefusal,
        plural_member_unfold::unfold_plural_members,
        temporal_validation_reason::TemporalValidationReason,
    };
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
    use serde_json::{Value, json};

    /// `attribute` unfolded on the `observedAt` axis, as a write unfolds it.
    fn unfolded(attribute: &Value) -> Result<Value, TemporalError> {
        let mut payload = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "attribute": attribute});
        unfold_plural_members(&mut payload, TimeProp::Observed)?;
        Ok(payload["attribute"].take())
    }

    /// The refusal `attribute` is refused with on the `observedAt` axis.
    fn refused(attribute: &Value) -> PluralMemberRefusal {
        match unfolded(attribute) {
            Err(TemporalError::Validation(TemporalValidationReason::PluralMember(refusal))) => refusal,
            other => panic!("not a plural member refusal: {other:?}"),
        }
    }

    /// Clause 4.5.9, EXAMPLE 1: each pair is the value and the instant it was
    /// observed at.
    #[test]
    fn each_pair_becomes_the_instance_it_stands_for() {
        let attribute = json!({"type": "Property", "values": [["Joe Bloggs", "2022-08-09T18:25:02Z"], ["Bill Smith", "2022-08-10T18:25:02Z"]]});
        pretty_assertions::assert_eq!(
            unfolded(&attribute).expect("the pairs unfold"),
            json!([
                {"type": "Property", "value": "Joe Bloggs", "observedAt": "2022-08-09T18:25:02Z"},
                {"type": "Property", "value": "Bill Smith", "observedAt": "2022-08-10T18:25:02Z"}
            ])
        );
    }

    /// Clause 4.5.9 holds every family's pairs in its own member, the value of
    /// a `LanguageProperty`, `JsonProperty` and `VocabProperty` wrapped in a
    /// one-member object (EXAMPLES 2 to 7); each unfolds to the family's value
    /// member holding the value alone.
    #[test]
    fn every_family_unfolds_into_its_own_value_member() {
        let instant = "2022-08-09T18:25:02Z";
        for (attribute, instance) in [
            (
                json!({"type": "GeoProperty", "values": [[{"type": "Point", "coordinates": [1, 2]}, instant]]}),
                json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2]}, "observedAt": instant}),
            ),
            (
                json!({"type": "Relationship", "objects": [[["urn:ngsi-ld:Device:1", "urn:ngsi-ld:Device:2"], instant]]}),
                json!({"type": "Relationship", "object": ["urn:ngsi-ld:Device:1", "urn:ngsi-ld:Device:2"], "observedAt": instant}),
            ),
            (
                json!({"type": "LanguageProperty", "languageMaps": [[{"languageMap": {"en": "yes", "fr": "oui"}}, instant]]}),
                json!({"type": "LanguageProperty", "languageMap": {"en": "yes", "fr": "oui"}, "observedAt": instant}),
            ),
            (
                json!({"type": "JsonProperty", "jsons": [[{"json": [{"id": "85a6cc52"}]}, instant]]}),
                json!({"type": "JsonProperty", "json": [{"id": "85a6cc52"}], "observedAt": instant}),
            ),
            (
                json!({"type": "VocabProperty", "vocabs": [[{"vocab": "Male"}, instant]]}),
                json!({"type": "VocabProperty", "vocab": "Male", "observedAt": instant}),
            ),
            (
                json!({"type": "ListProperty", "valueLists": [[["First", "Second"], instant]]}),
                json!({"type": "ListProperty", "valueList": ["First", "Second"], "observedAt": instant}),
            ),
            (
                json!({"type": "ListRelationship", "objectLists": [[["urn:ngsi-ld:Person:Alice"], instant]]}),
                json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Person:Alice"], "observedAt": instant}),
            ),
        ] {
            pretty_assertions::assert_eq!(unfolded(&attribute).expect("the pairs unfold"), json!([instance]));
        }
    }

    /// Clause 4.5.5: a temporal representation gives each `datasetId` its own
    /// object, whose pairs are instances of that dataset; an instance written
    /// in full beside them is left as it is.
    #[test]
    fn every_pair_takes_the_dataset_of_its_object() {
        let attribute = json!([
            {"type": "Property", "datasetId": "urn:ngsi-ld:Dataset:a", "values": [[1, "2022-08-09T18:25:02Z"], [2, "2022-08-10T18:25:02Z"]]},
            {"type": "Property", "value": 3, "observedAt": "2022-08-11T18:25:02Z"}
        ]);
        pretty_assertions::assert_eq!(
            unfolded(&attribute).expect("the pairs unfold"),
            json!([
                {"type": "Property", "datasetId": "urn:ngsi-ld:Dataset:a", "value": 1, "observedAt": "2022-08-09T18:25:02Z"},
                {"type": "Property", "datasetId": "urn:ngsi-ld:Dataset:a", "value": 2, "observedAt": "2022-08-10T18:25:02Z"},
                {"type": "Property", "value": 3, "observedAt": "2022-08-11T18:25:02Z"}
            ])
        );
    }

    /// On a system time axis the instant is the broker's, so a source's answer
    /// paired on `modifiedAt` unfolds to instances carrying no instant.
    #[test]
    fn a_pair_made_on_a_system_time_keeps_no_instant() {
        let mut payload = json!({"id": "urn:ngsi-ld:Vehicle:1", "speed": {"type": "Property", "values": [[60, "2020-08-01T12:00:00Z"]]}});
        unfold_plural_members(&mut payload, TimeProp::Modified).expect("the pairs unfold");
        pretty_assertions::assert_eq!(payload["speed"], json!([{"type": "Property", "value": 60}]));
    }

    /// Clause 4.5.9 has the object "only contain" its plural member: an
    /// instance member beside the pairs, which no element could carry, is
    /// refused rather than copied into every instance.
    #[test]
    fn a_member_beside_the_pairs_is_refused() {
        for attribute in [
            json!({"type": "Property", "unitCode": "KMH", "values": [[60, "2020-08-01T12:00:00Z"]]}),
            json!({"type": "Property", "accuracy": {"type": "Property", "value": 1}, "values": [[60, "2020-08-01T12:00:00Z"]]}),
        ] {
            assert!(matches!(refused(&attribute), PluralMemberRefusal::MemberBesidePairs { .. }), "{attribute}");
        }
    }

    /// Every element of the plural member is a pair: an instance object, a
    /// bare value and a pair missing or adding an element are refused.
    #[test]
    fn an_element_that_is_no_pair_is_refused() {
        for element in [
            json!({"value": 60, "observedAt": "2020-08-01T12:00:00Z"}),
            json!(60),
            json!([60]),
            json!([60, "2020-08-01T12:00:00Z", "2020-08-01T13:00:00Z"]),
        ] {
            let attribute = json!({"type": "Property", "values": [element]});
            assert!(matches!(refused(&attribute), PluralMemberRefusal::NotAPair { .. }), "{element}");
        }
        let attribute = json!({"type": "Property", "values": [60, "2020-08-01T12:00:00Z"]});
        assert!(matches!(refused(&attribute), PluralMemberRefusal::NotAPair { .. }));
        let attribute = json!({"type": "Property", "values": {"value": 60}});
        assert!(matches!(refused(&attribute), PluralMemberRefusal::NotAnArray { .. }));
    }

    /// A wrapped family's value is refused unwrapped, wrapped under another
    /// member, or wrapped beside a second member.
    #[test]
    fn a_wrapped_value_outside_its_one_member_object_is_refused() {
        for element in [
            json!({"en": "yes"}),
            json!({"value": {"en": "yes"}}),
            json!({"languageMap": {"en": "yes"}, "observedAt": "2020-08-01T12:00:00Z"}),
        ] {
            let attribute = json!({"type": "LanguageProperty", "languageMaps": [[element, "2020-08-01T12:00:00Z"]]});
            assert!(matches!(refused(&attribute), PluralMemberRefusal::UnwrappedValue { .. }), "{element}");
        }
    }

    /// On the `observedAt` axis the instant is the instance's `observedAt`,
    /// which is a date-time string.
    #[test]
    fn an_instant_that_is_no_string_is_refused() {
        let attribute = json!({"type": "Property", "values": [[60, null]]});
        assert!(matches!(refused(&attribute), PluralMemberRefusal::InstantNotAString { .. }));
    }

    /// An object naming no family, or not holding its family's member, is an
    /// instance and is left for the expansion to read.
    #[test]
    fn an_object_without_its_family_plural_member_is_left_alone() {
        for attribute in [
            json!({"type": "Property", "value": 60, "observedAt": "2020-08-01T12:00:00Z"}),
            json!({"type": "Relationship", "values": [[60, "2020-08-01T12:00:00Z"]]}),
            json!({"values": [[60, "2020-08-01T12:00:00Z"]]}),
        ] {
            pretty_assertions::assert_eq!(unfolded(&attribute).expect("nothing unfolds"), attribute);
        }
    }
}
