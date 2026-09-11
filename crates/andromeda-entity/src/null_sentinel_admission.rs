//! Where a write payload may carry the NGSI-LD Null, and where it may not.
//!
//! Clause 5.2.1 states the default: "in the context of a partial update or
//! merge operation (see clauses 5.5.8 and 5.5.12), an NGSI-LD Null shall be
//! used to indicate the removal of a target member, as explained in clause
//! 4.5.0. In all other cases, implementations shall raise an error of type
//! `BadRequestData` if an NGSI-LD Null value is encountered."
//!
//! Clause 5.5.4 turns that into three prohibitions, and they do not share one
//! exception list:
//!
//! - `"<key>": "urn:ngsi-ld:null"` as a first level member value, excepted for
//!   NGSI-LD Fragments "used in partial update and merge operations".
//! - `urn:ngsi-ld:null` as the right-hand side of a Property `value`, of a
//!   Relationship `object`, or as `{"@none": "urn:ngsi-ld:null"}` on a
//!   `languageMap`, excepted for NGSI-LD Fragments "used in update and merge
//!   operations". Clauses 4.5.21.2 and 4.5.22.2 give the two list families
//!   their own spelling of the same sentinel, in `valueList` and `objectList`.
//! - `urn:ngsi-ld:null` as "the value of a key value pair within a JSON object,
//!   which is the right-hand side of the value of a Property", excepted only
//!   for "NGSI-LD Fragments used in merge operations (see clause 5.5.12)". A
//!   partial update is not exempt from this one.
//!
//! Clauses 5.5.8 and 5.5.12 add a fourth rule that carries no exception at all:
//! "A `datasetId` cannot be deleted by setting it to the value
//! `urn:ngsi-ld:null`." A `datasetId` holding the sentinel would also be served
//! back to clients as a first level member value, which the first prohibition
//! forbids outright.
//!
//! Which prohibitions an operation is exempt from is named by
//! [`NullSentinelAdmission`]. An entity-shaped payload and the bare Attribute
//! Fragment of clauses 5.6.4 and 5.6.19 are entered through their own function,
//! because the top-level node of the one is an Entity whose members are
//! attributes, and of the other a single attribute instance.

use crate::error::EntityError;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::expanded::{
    json_literal_merge::json_object_carries_null_sentinel,
    node::expanded_nodes,
    null_sentinel::{instance_is_null_sentinel, member_is_null_sentinel},
    reserved_member::is_reserved_member,
};
use andromeda_ngsi_ld_representation::family_member_admission::MemberAdmission;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use serde_json::{Map, Value};

/// Which of the clause 5.5.4 NGSI-LD Null prohibitions the operation writing a
/// payload is exempt from.
///
/// Which operation takes which is [`crate::write_admission`]'s table, and that
/// is what a caller outside this crate names; this enum is the crate's own
/// vocabulary for the answer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NullSentinelAdmission {
    /// None of them: an NGSI-LD Null anywhere in the payload is
    /// `BadRequestData` (clause 5.2.1).
    ///
    /// Create Entity (clause 5.6.1), Append Attributes (clause 5.6.3), Replace
    /// Entity (clause 5.6.18, whose clause 5.6.18.4 states that Replace does
    /// not support nulls), Replace Attribute (clause 5.6.19), Batch Entity
    /// Creation (clause 5.6.7), Batch Entity Creation or Update (clause 5.6.8)
    /// and Batch Entity Update (clause 5.6.9) all submit a whole Entity or a
    /// whole Attribute rather than a Fragment. Clause 5.6.8.4 has the same
    /// payload create the Entity when it does not exist — where a deletion
    /// sentinel has nothing to delete — and clause 5.6.9.4 runs "the behaviour
    /// defined by clause 5.6.3" locally, counting a success as "(Attributes
    /// appended)".
    ///
    /// Only clauses 5.6.2, 5.6.4 and 5.6.17 carry the sentence "NGSI-LD Nulls
    /// should be supported by this operation"; clause 5.6.18 carries its
    /// negation, and every other operation is silent, which clause 5.5.4
    /// resolves by excepting fragments used in partial update and merge
    /// operations alone.
    Refused,
    /// The first two: a partial update Fragment (clause 5.5.8) deletes a target
    /// member by setting it to the sentinel, both at the first level and in the
    /// value position.
    ///
    /// The nested prohibition still applies, because clause 5.5.4 excepts merge
    /// operations from that one alone. Update Attributes (clause 5.6.2, whose
    /// deletion by null is clause 5.5.8 EXAMPLE 3) and Partial Update Attribute
    /// (clause 5.6.4) take this — the two operations that say so themselves.
    PartialUpdateFragment,
    /// All three: a merge Fragment (clause 5.5.12) merges "up to an arbitrary
    /// depth, e.g. including going into JSON objects representing a Property
    /// value", so a sentinel nested inside a Property value object deletes the
    /// key it stands on (clause 5.5.12 EXAMPLE 2).
    ///
    /// Merge Entity (clause 5.6.17) and Batch Entity Merge (clause 5.6.20) take
    /// this; clause 5.6.20.4's local phase runs "the behaviour defined by
    /// clause 5.6.17", counting a success as "(Attributes updated, appended or
    /// deleted)".
    MergeFragment,
}

impl NullSentinelAdmission {
    /// Whether a first-level member or a value-position member may be the
    /// sentinel (the first two prohibitions of clause 5.5.4).
    const fn deletion_sentinel_admission(self) -> MemberAdmission {
        match self {
            NullSentinelAdmission::Refused => MemberAdmission::Forbidden,
            NullSentinelAdmission::PartialUpdateFragment | NullSentinelAdmission::MergeFragment => MemberAdmission::Admitted,
        }
    }

    /// Whether a key inside the JSON object on the right-hand side of a
    /// Property `value` may be the sentinel (the third prohibition of clause
    /// 5.5.4, excepted for merge operations alone).
    const fn nested_sentinel_admission(self) -> MemberAdmission {
        match self {
            NullSentinelAdmission::Refused | NullSentinelAdmission::PartialUpdateFragment => MemberAdmission::Forbidden,
            NullSentinelAdmission::MergeFragment => MemberAdmission::Admitted,
        }
    }
}

/// Reject the NGSI-LD Null placements an entity-shaped expanded payload may not
/// carry under `admission`.
///
/// # Errors
///
/// Reports [`EntityError::Validation`], which the HTTP binding answers with the
/// `BadRequestData` of clause 5.5.4, for every prohibition `admission` does not
/// lift, and for a `datasetId` set to the sentinel under any admission.
pub(crate) fn reject_null_placements_in_entity(expanded: &Value, admission: NullSentinelAdmission) -> Result<(), EntityError> {
    for entity in expanded_nodes(expanded) {
        let Some(members) = entity.as_object() else {
            continue;
        };
        reject_in_attribute_members(members, admission)?;
    }
    Ok(())
}

/// Reject the NGSI-LD Null placements the bare Attribute Fragment of clauses
/// 5.6.4 and 5.6.19 may not carry under `admission`.
///
/// The top-level node of such a payload is one attribute instance rather than
/// an Entity, so `attribute` names what the fragment is about; the request
/// carries it in the path rather than in the body.
///
/// # Errors
///
/// Reports [`EntityError::Validation`] on the same terms as
/// [`reject_null_placements_in_entity`].
pub(crate) fn reject_null_placements_in_attribute(expanded: &Value, attribute: &AttributeName, admission: NullSentinelAdmission) -> Result<(), EntityError> {
    for instance in expanded_nodes(expanded) {
        reject_in_instance(attribute, instance, admission)?;
    }
    Ok(())
}

/// Walk the attribute members of an Entity, or the sub-attribute members of one
/// attribute instance: both hold an array of instances under an expanded
/// attribute IRI, and both are bound by the same prohibitions.
fn reject_in_attribute_members(members: &Map<String, Value>, admission: NullSentinelAdmission) -> Result<(), EntityError> {
    for (key, member) in members {
        if key.starts_with('@') {
            continue;
        }
        let attribute = key
            .parse::<AttributeName>()
            .map_err(|error| EntityError::validation(format!("invalid expanded attribute name '{key}': {error}")))?;
        if is_reserved_member(&attribute) {
            continue;
        }
        let Some(instances) = member.as_array() else {
            continue;
        };
        for instance in instances {
            reject_in_instance(&attribute, instance, admission)?;
        }
    }
    Ok(())
}

/// Apply every prohibition to one attribute instance and to its sub-attributes.
fn reject_in_instance(attribute: &AttributeName, instance: &Value, admission: NullSentinelAdmission) -> Result<(), EntityError> {
    if dataset_id_is_sentinel(instance) {
        return Err(EntityError::validation(format!(
            "attribute '{attribute}' sets its datasetId to 'urn:ngsi-ld:null'; a datasetId cannot be deleted by setting it to that value (clauses 5.5.8 and 5.5.12)"
        )));
    }
    if admission.deletion_sentinel_admission() == MemberAdmission::Forbidden && (member_is_null_sentinel(instance) || instance_is_null_sentinel(instance)) {
        return Err(EntityError::validation(format!(
            "attribute '{attribute}' carries the reserved value 'urn:ngsi-ld:null', which this operation does not permit (clause 5.5.4)"
        )));
    }
    if admission.nested_sentinel_admission() == MemberAdmission::Forbidden && value_object_carries_sentinel(instance) {
        return Err(EntityError::validation(format!(
            "attribute '{attribute}' carries 'urn:ngsi-ld:null' inside the JSON object on the right-hand side of its value, which only a merge fragment may do (clause 5.5.4)"
        )));
    }
    let Some(members) = instance.as_object() else {
        return Ok(());
    };
    reject_in_attribute_members(members, admission)
}

/// Whether an attribute instance sets its `datasetId` to the sentinel.
fn dataset_id_is_sentinel(instance: &Value) -> bool {
    instance
        .get(properties::DATASET_ID.as_str())
        .is_some_and(|member| expanded_nodes(member).iter().any(member_is_null_sentinel))
}

/// Whether the JSON object on the right-hand side of a Property `value` holds
/// the sentinel under any of its own keys, at any depth.
///
/// The value node itself is not examined: a `value` that *is* the sentinel is
/// the second prohibition of clause 5.5.4, not the third.
///
/// A `JsonProperty` is examined too. Clause 4.5.24.2 prohibits `value` on that
/// family "as value is a generalization of json", so the object clause 5.5.4
/// describes as "the right-hand side of the value of a Property" is its `json`
/// member — and because clause 4.5.24.2 keeps that value "not available for
/// JSON-LD interpretation", the sentinel sits there as a bare string rather
/// than as an expanded node.
fn value_object_carries_sentinel(instance: &Value) -> bool {
    let property_value = instance
        .get(properties::VALUE.as_str())
        .is_some_and(|value| expanded_nodes(value).iter().any(node_members_carry_sentinel));
    property_value || json_literal_carries_sentinel(instance)
}

/// Whether the raw JSON of a `JsonProperty` holds the sentinel under any of its
/// own keys, at any depth.
fn json_literal_carries_sentinel(instance: &Value) -> bool {
    instance.get(properties::JSON.as_str()).is_some_and(|member| {
        expanded_nodes(member)
            .iter()
            .filter_map(|node| node.get("@value"))
            .any(json_object_carries_null_sentinel)
    })
}

/// Whether any non-keyword member of one expanded node holds the sentinel.
fn node_members_carry_sentinel(node: &Value) -> bool {
    node.as_object().is_some_and(|members| {
        members
            .iter()
            .filter(|(key, _)| !key.starts_with('@'))
            .any(|(_, member)| subtree_carries_sentinel(member))
    })
}

/// Whether the sentinel occurs anywhere within an expanded subtree.
fn subtree_carries_sentinel(node: &Value) -> bool {
    match node {
        Value::Array(items) => items.iter().any(subtree_carries_sentinel),
        Value::Object(members) => member_is_null_sentinel(node) || members.values().any(subtree_carries_sentinel),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::EntityError,
        merge_options::MergeOptions,
        notification_recording_harness::{ENTITY_ID, entity_uri, live_target, recording_service},
        null_sentinel_admission::{NullSentinelAdmission, reject_null_placements_in_attribute, reject_null_placements_in_entity},
        side_effect::BatchSideEffectMode,
    };
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::entity::{Entity, EntityPatch};
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use serde_json::{Value, json};

    /// One expanded entity carrying a single `member` attribute.
    fn entity_with(member: &Value) -> Value {
        json!([{ "@id": "urn:ngsi-ld:Device:1", "https://example.org/member": member }])
    }

    /// The attribute name a bare Attribute Fragment is written under.
    fn attribute() -> AttributeName {
        "https://example.org/member".parse().expect("the fixture names a valid attribute name")
    }

    /// Every admission, so a case can state what each one does with a payload.
    const ADMISSIONS: [NullSentinelAdmission; 3] = [
        NullSentinelAdmission::Refused,
        NullSentinelAdmission::PartialUpdateFragment,
        NullSentinelAdmission::MergeFragment,
    ];

    #[test]
    fn a_property_value_of_the_null_sentinel_is_refused_only_outside_a_fragment() {
        let member = json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            expanded_properties::VALUE.as_str(): [{"@value": "urn:ngsi-ld:null"}]
        }]);
        let error =
            reject_null_placements_in_entity(&entity_with(&member), NullSentinelAdmission::Refused).expect_err("clause 5.5.4 forbids a null value here");
        assert!(error.to_string().contains("urn:ngsi-ld:null"), "{error}");
        for admission in [NullSentinelAdmission::PartialUpdateFragment, NullSentinelAdmission::MergeFragment] {
            reject_null_placements_in_entity(&entity_with(&member), admission).expect("a fragment deletes a member with the sentinel");
        }
    }

    #[test]
    fn a_relationship_object_of_the_null_sentinel_is_refused_only_outside_a_fragment() {
        let member = json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
            expanded_properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:null"}]
        }]);
        let error =
            reject_null_placements_in_entity(&entity_with(&member), NullSentinelAdmission::Refused).expect_err("clause 5.5.4 forbids a null object here");
        assert!(error.to_string().contains("urn:ngsi-ld:null"), "{error}");
        reject_null_placements_in_entity(&entity_with(&member), NullSentinelAdmission::PartialUpdateFragment)
            .expect("a fragment deletes a Relationship with the sentinel");
    }

    #[test]
    fn a_bare_first_level_null_member_is_refused_only_outside_a_fragment() {
        let member = json!([{"@value": "urn:ngsi-ld:null"}]);
        let error = reject_null_placements_in_entity(&entity_with(&member), NullSentinelAdmission::Refused).expect_err("a bare null member is forbidden");
        assert!(error.to_string().contains("urn:ngsi-ld:null"), "{error}");
        reject_null_placements_in_entity(&entity_with(&member), NullSentinelAdmission::MergeFragment)
            .expect("a merge fragment deletes the whole member with a bare sentinel");
    }

    /// Clauses 4.5.21.2 and 4.5.22.2: the list families spell their deletion as
    /// an array holding one single NGSI-LD Null, in `valueList` or `objectList`.
    #[test]
    fn a_list_family_deletion_sentinel_is_refused_only_outside_a_fragment() {
        for (family, carrier) in [
            ("https://uri.etsi.org/ngsi-ld/ListProperty", expanded_properties::VALUE_LIST.as_str()),
            ("https://uri.etsi.org/ngsi-ld/ListRelationship", expanded_properties::OBJECT_LIST.as_str()),
        ] {
            let member = json!([{"@type": [family], carrier: [{"@list": [{"@value": "urn:ngsi-ld:null"}]}]}]);
            let error = reject_null_placements_in_entity(&entity_with(&member), NullSentinelAdmission::Refused)
                .expect_err("clause 5.5.4 forbids a list deletion outside a fragment");
            assert!(error.to_string().contains("urn:ngsi-ld:null"), "{family}: {error}");
            reject_null_placements_in_entity(&entity_with(&member), NullSentinelAdmission::PartialUpdateFragment)
                .expect("a fragment deletes a list family with the sentinel");
        }
    }

    /// Clause 5.5.4: the nested prohibition is excepted for "NGSI-LD Fragments
    /// used in merge operations (see clause 5.5.12)" alone, so a partial update
    /// is bound by it exactly as a creation is.
    #[test]
    fn a_null_nested_in_a_property_value_object_is_refused_outside_a_merge() {
        let member = json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            expanded_properties::VALUE.as_str(): [{
                "https://example.org/street": [{"@value": "Pariser Platz"}],
                "https://example.org/country": [{"@value": "urn:ngsi-ld:null"}]
            }]
        }]);
        for admission in [NullSentinelAdmission::Refused, NullSentinelAdmission::PartialUpdateFragment] {
            let error =
                reject_null_placements_in_entity(&entity_with(&member), admission).expect_err("only a merge fragment may nest the sentinel in a value object");
            assert!(error.to_string().contains("urn:ngsi-ld:null"), "{admission:?}: {error}");
        }
        reject_null_placements_in_entity(&entity_with(&member), NullSentinelAdmission::MergeFragment)
            .expect("clause 5.5.12 EXAMPLE 2 deletes a value-object key with the sentinel");
    }

    /// Clauses 5.5.8 and 5.5.12: "A `datasetId` cannot be deleted by setting it
    /// to the value `urn:ngsi-ld:null`" — under every admission, fragment or not.
    #[test]
    fn a_dataset_id_of_the_null_sentinel_is_refused_under_every_admission() {
        let member = json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            expanded_properties::VALUE.as_str(): [{"@value": 9}],
            expanded_properties::DATASET_ID.as_str(): [{"@id": "urn:ngsi-ld:null"}]
        }]);
        for admission in ADMISSIONS {
            let error = reject_null_placements_in_entity(&entity_with(&member), admission).expect_err("a datasetId cannot be deleted by the sentinel");
            assert!(error.to_string().contains("datasetId"), "{admission:?}: {error}");
        }
    }

    #[test]
    fn an_ordinary_dataset_id_is_accepted_under_every_admission() {
        let member = json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            expanded_properties::VALUE.as_str(): [{"@value": 9}],
            expanded_properties::DATASET_ID.as_str(): [{"@id": "urn:ngsi-ld:Dataset:A"}]
        }]);
        for admission in ADMISSIONS {
            reject_null_placements_in_entity(&entity_with(&member), admission).expect("an ordinary datasetId is well formed");
        }
    }

    /// A sub-attribute is a Property in its own right (clause 4.5.5), so the
    /// value-position prohibition reaches it too.
    #[test]
    fn a_null_sub_attribute_value_is_refused_outside_a_fragment() {
        let member = json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            expanded_properties::VALUE.as_str(): [{"@value": 9}],
            "https://example.org/accuracy": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                expanded_properties::VALUE.as_str(): [{"@value": "urn:ngsi-ld:null"}]
            }]
        }]);
        let error = reject_null_placements_in_entity(&entity_with(&member), NullSentinelAdmission::Refused)
            .expect_err("clause 5.5.4 forbids a null sub-attribute value here");
        assert!(error.to_string().contains("accuracy"), "{error}");
    }

    #[test]
    fn an_ordinary_property_value_is_accepted_under_every_admission() {
        let member = json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            expanded_properties::VALUE.as_str(): [{"@value": "office"}]
        }]);
        for admission in ADMISSIONS {
            reject_null_placements_in_entity(&entity_with(&member), admission).expect("an ordinary value is well formed");
        }
    }

    /// Clause 5.6.19 takes a bare Attribute Fragment, whose top-level node is
    /// the attribute instance itself rather than an Entity.
    #[test]
    fn an_attribute_fragment_value_of_the_null_sentinel_is_refused_only_outside_a_fragment() {
        let fragment = json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            expanded_properties::VALUE.as_str(): [{"@value": "urn:ngsi-ld:null"}]
        }]);
        let error =
            reject_null_placements_in_attribute(&fragment, &attribute(), NullSentinelAdmission::Refused).expect_err("Replace Attribute does not support nulls");
        assert!(error.to_string().contains("urn:ngsi-ld:null"), "{error}");
        reject_null_placements_in_attribute(&fragment, &attribute(), NullSentinelAdmission::PartialUpdateFragment)
            .expect("clause 5.6.4 deletes the selected instance with the sentinel");
    }

    #[test]
    fn an_attribute_fragment_dataset_id_of_the_null_sentinel_is_refused() {
        let fragment = json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            expanded_properties::VALUE.as_str(): [{"@value": 9}],
            expanded_properties::DATASET_ID.as_str(): [{"@id": "urn:ngsi-ld:null"}]
        }]);
        for admission in ADMISSIONS {
            let error = reject_null_placements_in_attribute(&fragment, &attribute(), admission).expect_err("a datasetId cannot be deleted by the sentinel");
            assert!(error.to_string().contains("datasetId"), "{admission:?}: {error}");
        }
    }

    /// One `Building` carrying `attribute`, as a client writes it before
    /// expansion.
    fn written_entity(attribute: &str, node: &Value) -> Entity {
        serde_json::from_value(json!({"id": ENTITY_ID, "type": "Building", attribute: node})).expect("the fixture is a valid entity")
    }

    /// The same payload as a fragment, for the operations that take one.
    fn written_patch(attribute: &str, node: &Value) -> EntityPatch {
        serde_json::from_value(json!({attribute: node})).expect("the fixture is a valid entity fragment")
    }

    /// Clause 5.5.4, third bullet: a null nested inside the JSON object on the
    /// right-hand side of a Property `value` is `BadRequestData` outside a merge
    /// fragment. Written against the real core `@context`, so the expansion the
    /// checker walks is the one a request produces.
    #[tokio::test]
    async fn creating_an_entity_whose_property_value_object_nests_a_null_is_refused() {
        let (service, _recorded) = recording_service().await;
        let entity = written_entity("address", &json!({"type": "Property", "value": {"country": "urn:ngsi-ld:null"}}));
        let error = service
            .create(&live_target(), &entity, &ContextSpec::core_only())
            .await
            .expect_err("a creation may not nest the sentinel in a value object");
        assert!(matches!(error, EntityError::Validation { .. }), "{error}");
    }

    /// Clause 5.5.12 EXAMPLE 2 merges exactly that fragment, so the same
    /// payload the creation refuses must be accepted here.
    #[tokio::test]
    async fn merging_a_fragment_whose_property_value_object_nests_a_null_is_accepted() {
        let (service, _recorded) = recording_service().await;
        let patch = written_patch("address", &json!({"type": "Property", "value": {"country": "urn:ngsi-ld:null"}}));
        service
            .merge(&live_target(), &entity_uri(), &patch, &ContextSpec::core_only(), MergeOptions::default())
            .await
            .expect("clause 5.5.12 deletes a value-object key with the sentinel");
    }

    /// Clauses 5.5.8 and 5.5.12: a `datasetId` set to the sentinel is refused
    /// even by the merge that every other null in the fragment is admitted for.
    #[tokio::test]
    async fn merging_a_fragment_that_deletes_a_dataset_id_is_refused() {
        let (service, _recorded) = recording_service().await;
        let patch = written_patch("category", &json!({"type": "Property", "value": 9, "datasetId": "urn:ngsi-ld:null"}));
        let error = service
            .merge(&live_target(), &entity_uri(), &patch, &ContextSpec::core_only(), MergeOptions::default())
            .await
            .expect_err("a datasetId cannot be deleted by the sentinel");
        assert!(matches!(error, EntityError::Validation { .. }), "{error}");
    }

    /// Clause 5.6.7.4 runs the clause 5.5.4 validation for each item of the
    /// array, and clause 5.2.17 reports the failure against that entity alone.
    #[tokio::test]
    async fn a_batch_creation_refuses_the_sentinel_against_the_entity_that_carried_it() {
        let (service, _recorded) = recording_service().await;
        let entities = vec![written_entity("category", &json!({"type": "Property", "value": "urn:ngsi-ld:null"}))];
        let batch = service
            .batch_create(&live_target(), &entities, &ContextSpec::core_only(), BatchSideEffectMode::Inline)
            .await
            .expect("the batch itself completes");
        assert!(batch.success.is_empty(), "nothing may be created");
        pretty_assertions::assert_eq!(batch.errors.len(), 1);
        pretty_assertions::assert_eq!(batch.errors[0].0.as_str(), ENTITY_ID);
        assert!(matches!(batch.errors[0].1, EntityError::Validation { .. }), "{}", batch.errors[0].1);
    }

    /// Clause 5.6.20 is a merge, so the batch admits what clause 5.5.12 gives
    /// its fragments.
    #[tokio::test]
    async fn a_batch_merge_admits_the_deletion_sentinel() {
        let (service, _recorded) = recording_service().await;
        let entities = vec![written_entity("category", &json!({"type": "Property", "value": "urn:ngsi-ld:null"}))];
        let batch = service
            .batch_merge(&live_target(), &entities, &ContextSpec::core_only(), BatchSideEffectMode::Inline)
            .await
            .expect("the batch itself completes");
        assert!(
            batch.errors.is_empty(),
            "{:?}",
            batch.errors.iter().map(|(id, error)| format!("{id}: {error}")).collect::<Vec<_>>()
        );
        pretty_assertions::assert_eq!(batch.success.len(), 1);
    }

    /// Clauses 4.5.21.2 and 4.5.22.2 give the list families their own spelling
    /// of the sentinel — the array holding one single NGSI-LD Null of clause
    /// 4.5.0 — and clause 4.5.18 gives a `languageMap` the `{"@none": ...}`
    /// one. A creation admits none of them, whichever wire spelling the
    /// Context Producer wrote.
    #[tokio::test]
    async fn creating_an_entity_with_any_family_spelling_of_the_sentinel_is_refused() {
        let cases = [
            ("label", json!({"type": "LanguageProperty", "languageMap": {"@none": "urn:ngsi-ld:null"}})),
            ("tags", json!({"type": "ListProperty", "valueList": ["urn:ngsi-ld:null"]})),
            ("hasRooms", json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:null"]})),
            ("belongsTo", json!({"type": "Relationship", "object": "urn:ngsi-ld:null"})),
        ];
        for (attribute, node) in cases {
            let (service, _recorded) = recording_service().await;
            let outcome = service
                .create(&live_target(), &written_entity(attribute, &node), &ContextSpec::core_only())
                .await;
            match outcome {
                Ok(created) => panic!("{attribute}: a creation admits no NGSI-LD Null, yet {created} was stored"),
                Err(error) => assert!(matches!(error, EntityError::Validation { .. }), "{attribute}: {error}"),
            }
        }
    }
}
