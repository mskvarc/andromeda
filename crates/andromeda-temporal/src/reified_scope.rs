//! What one reified Scope instance of a Temporal Evolution states.
//!
//! Clause 4.5.6 represents the Scope of a Temporal Evolution as "a temporal
//! representation of a Property": its `value` holds the scope paths, or the
//! NGSI-LD Null that clause 4.18 reserves for a deleted scope, under the same
//! grammar as the Scope of an entity. An instance stating anything else is no
//! Scope, and clause 5.5.4 answers it with `BadRequestData` rather than the
//! write recording nothing in its place.

use crate::scope_observation::ScopeObservation;
use andromeda_ngsi_ld_entity::{
    document::{
        slot::Slot,
        value_carrier::ValueCarrier,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::ExpandedValue,
    literal::LiteralValue,
    scope_refusal::ScopeRefusal,
    scope_slot::{ScopeItem, scope_slot},
    value_mode::Fragment,
};

/// The Scope observations the `value` of one reified Scope instance states:
/// each path it names, or the end of the entity's Scope.
///
/// # Errors
///
/// Returns [`ScopeRefusal::NotAProperty`] for a value carried by any member
/// but a Property's `value`, [`ScopeRefusal::Empty`] for an instance carrying
/// no value, and the refusals of [`scope_slot`] for a value outside the clause
/// 4.18 grammar.
pub(crate) fn scope_observations(value: &ValueSlot) -> Result<Vec<ScopeObservation>, ScopeRefusal> {
    match value {
        ValueSlot::Set(WrittenValue::Value(values)) => match scope_slot(values.iter().map(scope_item))? {
            Slot::Set(scopes) => Ok(scopes.as_slice().iter().cloned().map(ScopeObservation::Path).collect()), // clone: each observation owns its path.
            Slot::Remove => Ok(vec![ScopeObservation::Deleted]),
        },
        ValueSlot::Remove(ValueCarrier::Value) => Ok(vec![ScopeObservation::Deleted]),
        ValueSlot::Absent => Err(ScopeRefusal::Empty),
        ValueSlot::Set(
            WrittenValue::Geometry(_)
            | WrittenValue::Object(_)
            | WrittenValue::LanguageMap(_)
            | WrittenValue::Vocab(_)
            | WrittenValue::Json(_)
            | WrittenValue::ValueList(_)
            | WrittenValue::ObjectList(_),
        )
        | ValueSlot::Remove(
            ValueCarrier::Object | ValueCarrier::LanguageMap | ValueCarrier::Vocab | ValueCarrier::Json | ValueCarrier::ValueList | ValueCarrier::ObjectList,
        ) => Err(ScopeRefusal::NotAProperty),
    }
}

/// One value of a reified Scope instance, as a `scope` member item.
fn scope_item(value: &ExpandedValue<Fragment>) -> ScopeItem<'_> {
    let ExpandedValue::Literal(literal) = value else {
        return ScopeItem::NotAString;
    };
    match literal.value() {
        LiteralValue::String(text) => ScopeItem::Text(text),
        LiteralValue::Number(_) | LiteralValue::Boolean(_) => ScopeItem::NotAString,
    }
}

#[cfg(test)]
mod tests {
    use crate::{reified_scope::scope_observations, scope_observation::ScopeObservation};
    use andromeda_ngsi_ld_entity::{
        document::{
            instance_document::InstanceDocument,
            written_value::{ValueSlot, WrittenValue},
        },
        non_empty::NonEmpty,
        scope_refusal::ScopeRefusal,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, scope::path::ScopePath, uri::Uri};
    use andromeda_ngsi_ld_vocabulary::expanded::properties;
    use serde_json::{Value, json};

    /// The reified Scope instance whose `value` is the expanded `value`.
    fn instance(value: &Value) -> InstanceDocument {
        let scope: AttributeName = properties::SCOPE.as_str().parse().expect("valid attribute name");
        let instance = json!({"@type": ["https://uri.etsi.org/ngsi-ld/Property"], properties::VALUE.as_str(): value});
        InstanceDocument::from_slice(&scope, &serde_json::to_vec(&instance).expect("JSON")).expect("a reified scope instance decodes")
    }

    fn path(raw: &str) -> ScopeObservation {
        ScopeObservation::Path(raw.parse::<ScopePath>().expect("a valid scope path"))
    }

    /// Clause 4.5.6: the `value` of a reified Scope names its paths.
    #[test]
    fn each_path_the_value_names_is_observed() {
        let instance = instance(&json!([{"@value": "/Madrid/Gardens"}, {"@value": "CompanyA"}]));
        pretty_assertions::assert_eq!(scope_observations(instance.value()), Ok(vec![path("/Madrid/Gardens"), path("CompanyA")]));
    }

    /// Clause 4.18 reserves the NGSI-LD Null for a deleted scope.
    #[test]
    fn the_null_records_the_end_of_the_scope() {
        let instance = instance(&json!([{"@value": "urn:ngsi-ld:null"}]));
        pretty_assertions::assert_eq!(scope_observations(instance.value()), Ok(vec![ScopeObservation::Deleted]));
    }

    /// Clause 4.18 grammar: `-` is no `ScopeLevelChar`, so `/a-b` is no Scope.
    #[test]
    fn a_value_outside_the_scope_grammar_is_refused() {
        let instance = instance(&json!([{"@value": "/a-b"}]));
        assert!(
            matches!(scope_observations(instance.value()), Err(ScopeRefusal::NotAScopePath { .. })),
            "{:?}",
            scope_observations(instance.value())
        );
    }

    #[test]
    fn a_value_that_is_not_a_string_is_refused() {
        let instance = instance(&json!([{"@value": 7}]));
        pretty_assertions::assert_eq!(scope_observations(instance.value()), Err(ScopeRefusal::NotAString));
    }

    #[test]
    fn an_instance_carrying_no_value_is_refused() {
        pretty_assertions::assert_eq!(scope_observations(&ValueSlot::Absent), Err(ScopeRefusal::Empty));
    }

    /// Clause 4.5.6: the reified Scope is a Property, so it carries no
    /// Relationship `object`.
    #[test]
    fn a_value_carried_by_another_family_is_refused() {
        let object = ValueSlot::Set(WrittenValue::Object(NonEmpty::one(
            "urn:ngsi-ld:Scope:Madrid".parse::<Uri>().expect("a valid URI"),
        )));
        pretty_assertions::assert_eq!(scope_observations(&object), Err(ScopeRefusal::NotAProperty));
    }
}
