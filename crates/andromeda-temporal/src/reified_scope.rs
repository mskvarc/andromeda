//! What one reified Scope instance of a Temporal Evolution states.
//!
//! Clause 4.5.6 represents the Scope of a Temporal Evolution as "a temporal
//! representation of a Property" that "can only have the non-reified Temporal
//! Properties createdAt, modifiedAt, deletedAt and observedAt as
//! sub-Properties": its `value` holds the scope paths, or the NGSI-LD Null that
//! clause 4.18 reserves for a deleted scope, under the same grammar as the
//! Scope of an entity. An instance stating anything else is no Scope, and
//! clause 5.5.4 answers it with `BadRequestData` rather than the write
//! recording nothing in its place.

use crate::scope_observation::ScopeObservation;
use andromeda_ngsi_ld_entity::{
    document::{
        instance_document::InstanceDocument,
        slot::Slot,
        value_carrier::ValueCarrier,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::ExpandedValue,
    literal::LiteralValue,
    member_shape::MemberShape,
    scope_refusal::ScopeRefusal,
    scope_slot::{ScopeItem, scope_slot},
    value_mode::Fragment,
};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, scope::path::ScopePath};

/// The Scope observations one reified Scope instance states: each path its
/// `value` names, or the end of the entity's Scope.
///
/// # Errors
///
/// Returns [`ScopeRefusal::NotAProperty`] for an instance declaring a family
/// other than Property or carrying its value under any member but a
/// Property's `value`, [`ScopeRefusal::MemberForbidden`] and
/// [`ScopeRefusal::SubAttributeForbidden`] for a member clause 4.5.6 does not
/// give a reified Scope (deleting one with the NGSI-LD Null still carries it),
/// [`ScopeRefusal::Empty`] for an instance carrying no value, and the
/// refusals of [`scope_slot`] for a value outside the clause 4.18 grammar.
pub(crate) fn scope_observations(instance: &InstanceDocument) -> Result<Vec<ScopeObservation>, ScopeRefusal> {
    match instance.family() {
        None | Some(AttributeType::Property) => {}
        Some(
            AttributeType::Relationship
            | AttributeType::GeoProperty
            | AttributeType::VocabProperty
            | AttributeType::LanguageProperty
            | AttributeType::JsonProperty
            | AttributeType::ListProperty
            | AttributeType::ListRelationship,
        ) => return Err(ScopeRefusal::NotAProperty),
    }
    if let Some(member) = forbidden_member(instance) {
        return Err(ScopeRefusal::MemberForbidden { member });
    }
    if let Some(attribute) = instance.sub_attributes().keys().next() {
        return Err(ScopeRefusal::SubAttributeForbidden { attribute: attribute.clone() }); // clone: the refusal outlives the borrowed instance.
    }
    value_observations(instance.value())
}

/// The first member the instance carries that is none of the four Temporal
/// Properties clause 4.5.6 gives a reified Scope, nor its `value`.
fn forbidden_member(instance: &InstanceDocument) -> Option<MemberShape> {
    [
        instance.dataset_id().map(|_| MemberShape::DatasetId),
        instance.instance_id().map(|_| MemberShape::InstanceId),
        instance.expires_at().map(|_| MemberShape::ExpiresAt),
        instance.unit_code().map(|_| MemberShape::UnitCode),
        instance.value_type().map(|_| MemberShape::ValueType),
        instance.object_types().map(|_| MemberShape::ObjectType),
        (!instance.proof_seals().is_empty()).then_some(MemberShape::ProofSeal),
    ]
    .into_iter()
    .flatten()
    .next()
}

/// The Scope observations the `value` of a reified Scope instance states,
/// whatever other members the instance carries.
///
/// A Context Source's answer is read rather than admitted: clause 5.5.4
/// refuses a request, not what a source answers, so an extra member on its
/// Scope does not hide the paths the Scope names.
///
/// # Errors
///
/// As [`scope_observations`], less the refusals of the instance's members.
pub(crate) fn value_observations(value: &ValueSlot) -> Result<Vec<ScopeObservation>, ScopeRefusal> {
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

/// The paths every reified Scope instance of `instances` names; an instance
/// that ends the Scope names none, and one outside the clause 4.18 grammar
/// names nothing a filter could match.
pub(crate) fn reified_paths<'a>(instances: impl IntoIterator<Item = &'a InstanceDocument>) -> Vec<ScopePath> {
    instances
        .into_iter()
        .filter_map(|instance| value_observations(instance.value()).ok())
        .flatten()
        .filter_map(|observation| match observation {
            ScopeObservation::Path(path) => Some(path),
            ScopeObservation::Deleted => None,
        })
        .collect()
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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{reified_scope::scope_observations, scope_observation::ScopeObservation};
    use andromeda_ngsi_ld_entity::{
        document::{
            instance_document::InstanceDocument,
            slot::Slot,
            written_value::{ValueSlot, WrittenValue},
        },
        member_shape::MemberShape,
        non_empty::NonEmpty,
        proof_seals::ProofSeals,
        scope_refusal::ScopeRefusal,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, scope::path::ScopePath, uri::Uri};
    use andromeda_ngsi_ld_vocabulary::expanded::properties;
    use serde_json::{Value, json};

    /// The reified Scope instance decoded from the expanded `members`.
    fn decoded(members: &Value) -> InstanceDocument {
        let scope: AttributeName = properties::SCOPE.as_str().parse().expect("valid attribute name");
        InstanceDocument::from_slice(&scope, &serde_json::to_vec(members).expect("JSON")).expect("a reified scope instance decodes")
    }

    /// The reified Scope instance whose `value` is the expanded `value`.
    fn instance(value: &Value) -> InstanceDocument {
        decoded(&json!({"@type": ["https://uri.etsi.org/ngsi-ld/Property"], properties::VALUE.as_str(): value}))
    }

    fn path(raw: &str) -> ScopeObservation {
        ScopeObservation::Path(raw.parse::<ScopePath>().expect("a valid scope path"))
    }

    fn identifier(raw: &str) -> Identifier {
        raw.parse().expect("a valid identifier")
    }

    /// Clause 4.5.6: the `value` of a reified Scope names its paths.
    #[test]
    fn each_path_the_value_names_is_observed() {
        let instance = instance(&json!([{"@value": "/Madrid/Gardens"}, {"@value": "CompanyA"}]));
        pretty_assertions::assert_eq!(scope_observations(&instance), Ok(vec![path("/Madrid/Gardens"), path("CompanyA")]));
    }

    /// Clause 4.18 reserves the NGSI-LD Null for a deleted scope.
    #[test]
    fn the_null_records_the_end_of_the_scope() {
        let instance = instance(&json!([{"@value": "urn:ngsi-ld:null"}]));
        pretty_assertions::assert_eq!(scope_observations(&instance), Ok(vec![ScopeObservation::Deleted]));
    }

    /// Clause 4.18 grammar: `-` is no `ScopeLevelChar`, so `/a-b` is no Scope.
    #[test]
    fn a_value_outside_the_scope_grammar_is_refused() {
        let instance = instance(&json!([{"@value": "/a-b"}]));
        assert!(
            matches!(scope_observations(&instance), Err(ScopeRefusal::NotAScopePath { .. })),
            "{:?}",
            scope_observations(&instance)
        );
    }

    #[test]
    fn a_value_that_is_not_a_string_is_refused() {
        let instance = instance(&json!([{"@value": 7}]));
        pretty_assertions::assert_eq!(scope_observations(&instance), Err(ScopeRefusal::NotAString));
    }

    #[test]
    fn an_instance_carrying_no_value_is_refused() {
        pretty_assertions::assert_eq!(scope_observations(&InstanceDocument::builder().build()), Err(ScopeRefusal::Empty));
    }

    /// Clause 4.5.6: the reified Scope is a Property, so it carries no
    /// Relationship `object`.
    #[test]
    fn a_value_carried_by_another_family_is_refused() {
        let object = InstanceDocument::builder()
            .value(ValueSlot::Set(WrittenValue::Object(NonEmpty::one(
                "urn:ngsi-ld:Scope:Madrid".parse::<Uri>().expect("a valid URI"),
            ))))
            .build();
        pretty_assertions::assert_eq!(scope_observations(&object), Err(ScopeRefusal::NotAProperty));
    }

    /// Clause 4.5.6: a reified Scope is "a temporal representation of a
    /// Property", so declaring another family makes it no Scope.
    #[test]
    fn an_instance_declaring_a_relationship_is_refused() {
        let relationship = decoded(&json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
            properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:Scope:Madrid"}]
        }));
        pretty_assertions::assert_eq!(scope_observations(&relationship), Err(ScopeRefusal::NotAProperty));
    }

    /// Clause 4.5.6: a reified Scope "can only have the non-reified Temporal
    /// Properties createdAt, modifiedAt, deletedAt and observedAt as
    /// sub-Properties", so each other member is refused by name, and deleting
    /// one with the NGSI-LD Null still carries it.
    #[test]
    fn each_member_beyond_the_temporal_properties_is_refused_by_name() {
        let cases = [
            (
                MemberShape::DatasetId,
                InstanceDocument::builder()
                    .value(madrid())
                    .dataset_id(Some(identifier("urn:ngsi-ld:Dataset:a")))
                    .build(),
            ),
            (
                MemberShape::InstanceId,
                InstanceDocument::builder()
                    .value(madrid())
                    .instance_id(Some(identifier("urn:ngsi-ld:instance:1")))
                    .build(),
            ),
            (
                MemberShape::ExpiresAt,
                InstanceDocument::builder().value(madrid()).expires_at(Some(Slot::Remove)).build(),
            ),
            (
                MemberShape::UnitCode,
                InstanceDocument::builder().value(madrid()).unit_code(Some(Slot::Remove)).build(),
            ),
            (
                MemberShape::ValueType,
                InstanceDocument::builder().value(madrid()).value_type(Some(Slot::Remove)).build(),
            ),
            (
                MemberShape::ObjectType,
                InstanceDocument::builder().value(madrid()).object_types(Some(Slot::Remove)).build(),
            ),
            (
                MemberShape::ProofSeal,
                InstanceDocument::builder()
                    .value(madrid())
                    .proof_seals(ProofSeals::new(Some(Vec::new()), None))
                    .build(),
            ),
        ];
        for (member, instance) in cases {
            pretty_assertions::assert_eq!(scope_observations(&instance), Err(ScopeRefusal::MemberForbidden { member }), "{member}");
        }
    }

    /// The `value` of a reified Scope instance naming `/Madrid`.
    fn madrid() -> ValueSlot {
        instance(&json!([{"@value": "/Madrid"}])).into_parts().value
    }

    #[test]
    fn a_sub_attribute_is_refused_by_name() {
        let instance = decoded(&json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            properties::VALUE.as_str(): [{"@value": "/Madrid"}],
            "https://uri.etsi.org/ngsi-ld/default-context/sensorNote": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                properties::VALUE.as_str(): [{"@value": "north"}]
            }]
        }));
        pretty_assertions::assert_eq!(
            scope_observations(&instance),
            Err(ScopeRefusal::SubAttributeForbidden {
                attribute: "https://uri.etsi.org/ngsi-ld/default-context/sensorNote".parse().expect("valid attribute name"),
            })
        );
    }

    /// Clause 4.5.6 gives a reified Scope exactly the four Temporal
    /// Properties as sub-Properties.
    #[test]
    fn the_four_temporal_properties_are_admitted() {
        let instance = decoded(&json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            properties::VALUE.as_str(): [{"@value": "urn:ngsi-ld:null"}],
            properties::OBSERVED_AT.as_str(): [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2018-08-01T11:00:00Z"}],
            properties::CREATED_AT.as_str(): [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2018-08-01T10:00:00Z"}],
            properties::MODIFIED_AT.as_str(): [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2018-08-01T11:00:00Z"}],
            properties::DELETED_AT.as_str(): [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2018-08-01T11:00:00Z"}]
        }));
        pretty_assertions::assert_eq!(scope_observations(&instance), Ok(vec![ScopeObservation::Deleted]));
    }
}
