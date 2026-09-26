//! The observation a write leaves for the Temporal Evolution: the attribute
//! instances the write addressed, as they stand once it has landed.
//!
//! Clauses 4.5.7 and 4.5.8 compose the Temporal Evolution of an attribute of
//! "the sequence of instances of the referred Property", "each one representing
//! an instance of the Property (as mandated by clause 4.5.2)". A request
//! Fragment is not such an instance. Under clause 5.5.8 a partial update "only
//! changes the elements provided" (EXAMPLE 1 keeps the `unitCode` the Fragment
//! left out), under clause 5.5.12 a merge reaches into the value itself, and
//! clause 5.6.19.4 keeps the replaced instance's `createdAt`. What history
//! records is therefore read off the state the write produced, for exactly the
//! instances the write addressed and applied. An instance the write deleted is
//! absent from that state; its end is recorded by a tombstone instead.

use crate::expanded::{
    dataset_id::{InstanceDatasetIdError, get_instance_dataset_id, position_of_dataset},
    mutation::selection::{OverwritePolicy, addressed_dataset},
    node::{expanded_nodes, expanded_object_ref},
    null_sentinel::instance_is_null_sentinel,
    reserved_member::is_reserved_member,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use iri_rs::IriBuf;
use serde_json::{Map, Value};

/// The attribute instances one write addressed.
#[derive(Debug, Clone, Copy)]
pub enum AddressedInstances<'a> {
    /// Every instance an expanded Entity Fragment names, under the overwrite
    /// policy the write ran with (clauses 5.6.2, 5.6.3, 5.6.8, 5.6.9, 5.6.17,
    /// 5.6.18 and 5.6.20).
    EntityFragment {
        /// The expanded Entity Fragment the request carried.
        fragment: &'a Value,
        /// Whether instances the Entity already held were overwritten.
        overwrite: OverwritePolicy,
    },
    /// The one instance an Attribute Fragment addressed through its URL
    /// (clauses 5.6.4 and 5.6.19).
    AttributeFragment {
        /// Expanded name of the attribute the URL names.
        attribute: &'a IriBuf,
        /// The `datasetId` URL parameter, when the request carried one.
        dataset_id: Option<&'a Uri>,
        /// The expanded Attribute Fragment the request carried.
        fragment: &'a Value,
    },
}

/// The observation a write leaves for the Temporal Evolution, or `None` when
/// it applied no attribute instance and no Scope.
///
/// `previous` is the Entity before the write and `current` the Entity it
/// stored. The observation is an expanded Entity node carrying the `@id` and
/// `@type` of `current` and, for every instance `addressed` names that the
/// write applied, that instance as `current` holds it.
///
/// An instance is left out when the write did not apply it: under
/// [`OverwritePolicy::Preserve`] an instance the Entity already held is kept
/// as it was (clause 5.6.3.4), and an instance the write deleted, or never
/// stored because clause 5.6.2.4 has it ignored, is not in `current`. `scope`
/// is observed as `current` holds it, and under [`OverwritePolicy::Preserve`]
/// only when the write changed it.
///
/// # Errors
///
/// Returns [`InstanceDatasetIdError`] when an instance of the Fragment or of
/// either state carries a `datasetId` that is not an identifier, since which
/// instance it names cannot be told.
pub fn written_observation(addressed: AddressedInstances<'_>, previous: &Value, current: &Value) -> Result<Option<Value>, InstanceDatasetIdError> {
    let Some(current_node) = expanded_object_ref(current) else {
        return Ok(None);
    };
    let previous_node = expanded_object_ref(previous);
    let mut observed = Map::new();
    match addressed {
        AddressedInstances::EntityFragment { fragment, overwrite } => {
            let Some(fragment_node) = expanded_object_ref(fragment) else {
                return Ok(None);
            };
            for (member, requested) in fragment_node {
                if member == properties::SCOPE.as_str() {
                    if let Some(scope) = written_scope(current_node, previous_node, overwrite) {
                        observed.insert(member.clone(), scope.clone()); // clone: the observation owns the Scope the Entity holds.
                    }
                    continue;
                }
                if !names_attribute(member) {
                    continue;
                }
                let instances = written_instances(member, requested, current_node, previous_node, overwrite)?;
                if !instances.is_empty() {
                    observed.insert(member.clone(), Value::Array(instances)); // clone: the observation owns the attribute name.
                }
            }
        }
        AddressedInstances::AttributeFragment {
            attribute,
            dataset_id,
            fragment,
        } => {
            // Clause 5.5.8 hands a Fragment that is the NGSI-LD Null to clause
            // 5.6.5: the addressed instance is deleted, not observed.
            if !instance_is_null_sentinel(fragment) {
                let dataset = addressed_dataset(dataset_id, fragment)?;
                if let Some(instance) = instance_of(current_node, attribute.as_str(), dataset.as_ref())? {
                    observed.insert(attribute.as_str().to_owned(), Value::Array(vec![observed_instance(instance, fragment)]));
                }
            }
        }
    }
    if observed.is_empty() {
        return Ok(None);
    }
    for keyword in ["@id", "@type"] {
        if let Some(value) = current_node.get(keyword) {
            observed.insert(keyword.to_owned(), value.clone()); // clone: the observation owns the Entity's identity members.
        }
    }
    Ok(Some(Value::Array(vec![Value::Object(observed)])))
}

/// Whether an expanded Entity member names an attribute, rather than a JSON-LD
/// keyword or a member NGSI-LD reserves for itself.
fn names_attribute(member: &str) -> bool {
    !member.starts_with('@') && member.parse::<AttributeName>().is_ok_and(|name| !is_reserved_member(&name))
}

/// The resulting instances of one attribute the Entity Fragment named and the
/// write applied, in the Fragment's order.
fn written_instances(
    member: &str,
    requested: &Value,
    current_node: &Map<String, Value>,
    previous_node: Option<&Map<String, Value>>,
    overwrite: OverwritePolicy,
) -> Result<Vec<Value>, InstanceDatasetIdError> {
    let mut written = Vec::new();
    for requested_instance in expanded_nodes(requested) {
        if instance_is_null_sentinel(requested_instance) {
            continue;
        }
        let dataset = get_instance_dataset_id(requested_instance)?;
        // Clause 5.6.3.4 leaves an instance the Entity already held untouched
        // when overwriting is disabled, so the write observed nothing of it.
        if overwrite == OverwritePolicy::Preserve
            && previous_node
                .map(|node| instance_of(node, member, dataset.as_ref()))
                .transpose()?
                .flatten()
                .is_some()
        {
            continue;
        }
        if let Some(instance) = instance_of(current_node, member, dataset.as_ref())? {
            written.push(observed_instance(instance, requested_instance));
        }
    }
    Ok(written)
}

/// The Scope the write left on the Entity, when it is one the write applied.
fn written_scope<'a>(current_node: &'a Map<String, Value>, previous_node: Option<&Map<String, Value>>, overwrite: OverwritePolicy) -> Option<&'a Value> {
    let scope = current_node.get(properties::SCOPE.as_str())?;
    let unchanged = previous_node.and_then(|node| node.get(properties::SCOPE.as_str())) == Some(scope);
    // Clause 5.6.3.4 appends only "the Scopes in the Fragment that are not part
    // of the value of scope of the target Entity" when overwriting is disabled,
    // so a Scope that came out unchanged is one the write did not apply.
    (overwrite == OverwritePolicy::Overwrite || !unchanged).then_some(scope)
}

/// The instance of `member` with `dataset` as its `datasetId` (the default
/// instance for `None`, clause 4.5.5) in one expanded Entity node.
fn instance_of<'a>(node: &'a Map<String, Value>, member: &str, dataset: Option<&Uri>) -> Result<Option<&'a Value>, InstanceDatasetIdError> {
    let Some(value) = node.get(member) else {
        return Ok(None);
    };
    let instances = expanded_nodes(value);
    Ok(position_of_dataset(instances, dataset)?.and_then(|position| instances.get(position)))
}

/// One resulting instance as the observation records it.
///
/// Clause 4.5.7 has a system "maintain an instanceId for each such Property
/// instance", and every write adds a new instance to the Temporal Evolution.
/// An `instanceId` the stored instance still carries names the instance an
/// earlier write added, so the observation keeps only the one the request's
/// own Fragment supplied; without one, the instance is given a fresh identity
/// when it is recorded.
fn observed_instance(resulting: &Value, requested: &Value) -> Value {
    let mut instance = resulting.clone(); // clone: the observation owns a copy of the instance the Entity holds.
    if let Some(members) = instance.as_object_mut() {
        members.remove(properties::INSTANCE_ID.as_str());
        if let Some(instance_id) = requested.get(properties::INSTANCE_ID.as_str()) {
            members.insert(properties::INSTANCE_ID.as_str().to_owned(), instance_id.clone()); // clone: the observation owns the requested identity.
        }
    }
    instance
}

#[cfg(test)]
mod tests {
    use crate::expanded::{
        mutation::selection::OverwritePolicy,
        written_observation::{AddressedInstances, written_observation},
    };
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use iri_rs::IriBuf;
    use serde_json::{Value, json};

    const ENTITY: &str = "urn:ngsi-ld:Building:1";
    const BUILDING: &str = "https://example.org/Building";
    const PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/Property";
    const HAS_VALUE: &str = "https://uri.etsi.org/ngsi-ld/hasValue";
    const UNIT_CODE: &str = "https://uri.etsi.org/ngsi-ld/unitCode";
    const OBSERVED_AT: &str = "https://uri.etsi.org/ngsi-ld/observedAt";
    const CREATED_AT: &str = "https://uri.etsi.org/ngsi-ld/createdAt";
    const DATASET_ID: &str = "https://uri.etsi.org/ngsi-ld/datasetId";
    const INSTANCE_ID: &str = "https://uri.etsi.org/ngsi-ld/instanceId";
    const SCOPE: &str = "https://uri.etsi.org/ngsi-ld/scope";
    const TEMPERATURE: &str = "https://example.org/temperature";
    const ADDRESS: &str = "https://example.org/address";
    const NAME: &str = "https://example.org/name";

    fn entity(members: &Value) -> Value {
        let mut node = json!({"@id": ENTITY, "@type": [BUILDING]});
        if let (Some(node), Some(members)) = (node.as_object_mut(), members.as_object()) {
            node.extend(members.clone());
        }
        json!([node])
    }

    fn property(value: &Value) -> Value {
        json!({"@type": [PROPERTY], HAS_VALUE: [{"@value": value}]})
    }

    fn with(mut instance: Value, member: &str, value: &Value) -> Value {
        if let Some(members) = instance.as_object_mut() {
            members.insert(member.to_owned(), value.clone());
        }
        instance
    }

    fn dataset(raw: &str) -> Value {
        json!([{"@id": raw}])
    }

    fn uri(raw: &str) -> Uri {
        raw.parse().expect("a valid URI")
    }

    fn iri(raw: &str) -> IriBuf {
        IriBuf::new(raw.to_owned()).expect("a valid IRI")
    }

    fn observed_members(observation: Option<Value>) -> Value {
        let observation = observation.expect("the write observed something");
        observation.as_array().and_then(|nodes| nodes.first()).cloned().expect("one observed node")
    }

    fn entity_fragment(fragment: &Value, overwrite: OverwritePolicy) -> AddressedInstances<'_> {
        AddressedInstances::EntityFragment { fragment, overwrite }
    }

    /// Clause 5.5.8 EXAMPLE 1: the partial update leaves `unitCode` in place, so
    /// the observed instance is the patched one carrying it, not the Fragment.
    #[test]
    fn a_partial_attribute_update_observes_the_patched_instance_with_its_untouched_unit_code() {
        let before = with(
            with(property(&json!(25)), UNIT_CODE, &json!([{"@value": "CEL"}])),
            OBSERVED_AT,
            &json!([{"@value": "2022-03-14T01:59:26.535Z"}]),
        );
        let after = with(
            with(property(&json!(100)), UNIT_CODE, &json!([{"@value": "CEL"}])),
            OBSERVED_AT,
            &json!([{"@value": "2022-03-14T13:00:00.000Z"}]),
        );
        let fragment = with(property(&json!(100)), OBSERVED_AT, &json!([{"@value": "2022-03-14T13:00:00.000Z"}]));
        let attribute = iri(TEMPERATURE);

        let observed = written_observation(
            AddressedInstances::AttributeFragment {
                attribute: &attribute,
                dataset_id: None,
                fragment: &fragment,
            },
            &entity(&json!({TEMPERATURE: [before]})),
            &entity(&json!({TEMPERATURE: [after]})),
        )
        .expect("readable datasetIds");

        pretty_assertions::assert_eq!(observed_members(observed), json!({"@id": ENTITY, "@type": [BUILDING], TEMPERATURE: [after]}));
    }

    /// Clause 5.6.19.4 keeps the replaced instance's `createdAt`, which only the
    /// stored state carries.
    #[test]
    fn a_replaced_attribute_is_observed_with_the_created_at_the_replacement_kept() {
        let created_at = json!([{"@value": "2022-01-01T00:00:00.000000Z"}]);
        let after = with(property(&json!("short")), CREATED_AT, &created_at);
        let attribute = iri(TEMPERATURE);

        let observed = written_observation(
            AddressedInstances::AttributeFragment {
                attribute: &attribute,
                dataset_id: None,
                fragment: &property(&json!("short")),
            },
            &entity(&json!({TEMPERATURE: [with(property(&json!("long")), CREATED_AT, &created_at)]})),
            &entity(&json!({TEMPERATURE: [after]})),
        )
        .expect("readable datasetIds");

        pretty_assertions::assert_eq!(observed_members(observed)[TEMPERATURE], json!([after]));
    }

    /// Only the addressed instance of a multi-attribute is observed; the other
    /// instance did not change (clause 4.5.5).
    #[test]
    fn an_attribute_fragment_observes_only_the_instance_its_dataset_id_addresses() {
        let first = with(property(&json!(1)), DATASET_ID, &dataset("urn:ngsi-ld:Dataset:a"));
        let second = with(property(&json!(2)), DATASET_ID, &dataset("urn:ngsi-ld:Dataset:b"));
        let fragment = with(property(&json!(2)), DATASET_ID, &dataset("urn:ngsi-ld:Dataset:b"));
        let attribute = iri(TEMPERATURE);
        let state = entity(&json!({TEMPERATURE: [first, second]}));

        let observed = written_observation(
            AddressedInstances::AttributeFragment {
                attribute: &attribute,
                dataset_id: None,
                fragment: &fragment,
            },
            &state,
            &state,
        )
        .expect("readable datasetIds");

        pretty_assertions::assert_eq!(observed_members(observed)[TEMPERATURE], json!([second]));
    }

    /// The `datasetId` URL parameter addresses ahead of a Fragment that names none.
    #[test]
    fn an_attribute_fragment_observes_the_instance_the_url_dataset_id_addresses() {
        let first = property(&json!(1));
        let second = with(property(&json!(2)), DATASET_ID, &dataset("urn:ngsi-ld:Dataset:b"));
        let attribute = iri(TEMPERATURE);
        let dataset_id = uri("urn:ngsi-ld:Dataset:b");
        let state = entity(&json!({TEMPERATURE: [first, second]}));

        let observed = written_observation(
            AddressedInstances::AttributeFragment {
                attribute: &attribute,
                dataset_id: Some(&dataset_id),
                fragment: &property(&json!(2)),
            },
            &state,
            &state,
        )
        .expect("readable datasetIds");

        pretty_assertions::assert_eq!(observed_members(observed)[TEMPERATURE], json!([second]));
    }

    /// Clause 5.5.8 hands an NGSI-LD Null Fragment to clause 5.6.5: a deletion
    /// is a tombstone, never an observation.
    #[test]
    fn an_attribute_fragment_that_deletes_the_instance_observes_nothing() {
        let attribute = iri(TEMPERATURE);
        let observed = written_observation(
            AddressedInstances::AttributeFragment {
                attribute: &attribute,
                dataset_id: None,
                fragment: &property(&json!("urn:ngsi-ld:null")),
            },
            &entity(&json!({TEMPERATURE: [property(&json!(1))]})),
            &entity(&json!({})),
        )
        .expect("readable datasetIds");

        assert!(observed.is_none(), "{observed:?}");
    }

    /// Clause 5.5.12 EXAMPLE 2: the merge keeps `city`, which the Fragment never
    /// restated, so the observed value is the merged one.
    #[test]
    fn a_merged_attribute_is_observed_with_the_value_members_the_fragment_left_out() {
        let merged_value = json!([{"@type": "@json", "@value": {"street": "Pariser Platz", "city": "Berlin"}}]);
        let after = json!({"@type": [PROPERTY], HAS_VALUE: merged_value});
        let fragment = entity(
            &json!({ADDRESS: [{"@type": [PROPERTY], HAS_VALUE: [{"@type": "@json", "@value": {"street": "Pariser Platz", "country": "urn:ngsi-ld:null"}}]}]}),
        );

        let observed = written_observation(
            entity_fragment(&fragment, OverwritePolicy::Overwrite),
            &entity(&json!({ADDRESS: [{"@type": [PROPERTY], HAS_VALUE: [{"@type": "@json", "@value": {"street": "Straße des 17. Juni", "city": "Berlin", "country": "Germany"}}]}]})),
            &entity(&json!({ADDRESS: [after]})),
        )
        .expect("readable datasetIds");

        pretty_assertions::assert_eq!(observed_members(observed)[ADDRESS], json!([after]));
    }

    /// Only the attributes the Fragment named are observed; the Entity's other
    /// attributes did not change.
    #[test]
    fn an_entity_fragment_observes_only_the_attributes_it_named() {
        let after_name = property(&json!("lobby"));
        let fragment = entity(&json!({NAME: [property(&json!("lobby"))]}));
        let current = entity(&json!({NAME: [after_name], TEMPERATURE: [property(&json!(21))]}));

        let observed = written_observation(
            entity_fragment(&fragment, OverwritePolicy::Overwrite),
            &entity(&json!({TEMPERATURE: [property(&json!(21))]})),
            &current,
        )
        .expect("readable datasetIds");

        pretty_assertions::assert_eq!(observed_members(observed), json!({"@id": ENTITY, "@type": [BUILDING], NAME: [after_name]}));
    }

    /// Clause 5.6.3.4: with overwriting disabled an instance the Entity already
    /// held is not appended, so it is not observed either; a new one is.
    #[test]
    fn a_no_overwrite_append_observes_only_the_instances_it_appended() {
        let kept = property(&json!(21));
        let appended = with(property(&json!(22)), DATASET_ID, &dataset("urn:ngsi-ld:Dataset:b"));
        let fragment = entity(&json!({TEMPERATURE: [property(&json!(99)), appended], NAME: [property(&json!("lobby"))]}));
        let previous = entity(&json!({TEMPERATURE: [kept], NAME: [property(&json!("hall"))]}));
        let current = entity(&json!({TEMPERATURE: [kept, appended], NAME: [property(&json!("hall"))]}));

        let observed = written_observation(entity_fragment(&fragment, OverwritePolicy::Preserve), &previous, &current).expect("readable datasetIds");

        pretty_assertions::assert_eq!(observed_members(observed), json!({"@id": ENTITY, "@type": [BUILDING], TEMPERATURE: [appended]}));
    }

    /// A no-overwrite append that appended nothing observes nothing.
    #[test]
    fn a_no_overwrite_append_that_appended_nothing_observes_nothing() {
        let state = entity(&json!({TEMPERATURE: [property(&json!(21))]}));
        let fragment = entity(&json!({TEMPERATURE: [property(&json!(99))]}));

        let observed = written_observation(entity_fragment(&fragment, OverwritePolicy::Preserve), &state, &state).expect("readable datasetIds");

        assert!(observed.is_none(), "{observed:?}");
    }

    /// An instance the Fragment deletes is absent from the stored state and is
    /// recorded by a tombstone, not observed.
    #[test]
    fn an_entity_fragment_does_not_observe_the_instance_it_deleted() {
        let fragment = entity(&json!({TEMPERATURE: [property(&json!("urn:ngsi-ld:null"))], NAME: [property(&json!("lobby"))]}));
        let current = entity(&json!({NAME: [property(&json!("lobby"))]}));

        let observed = written_observation(
            entity_fragment(&fragment, OverwritePolicy::Overwrite),
            &entity(&json!({TEMPERATURE: [property(&json!(1))], NAME: [property(&json!("hall"))]})),
            &current,
        )
        .expect("readable datasetIds");

        let members = observed_members(observed);
        assert!(members.get(TEMPERATURE).is_none(), "{members}");
        pretty_assertions::assert_eq!(members[NAME], json!([property(&json!("lobby"))]));
    }

    /// Clause 5.6.2.4 ignores a `scope` the target Entity does not hold, so the
    /// write observed no Scope.
    #[test]
    fn a_scope_the_update_ignored_is_not_observed() {
        let fragment = entity(&json!({SCOPE: [{"@value": "/Madrid"}]}));
        let state = entity(&json!({}));

        let observed = written_observation(entity_fragment(&fragment, OverwritePolicy::Overwrite), &state, &state).expect("readable datasetIds");

        assert!(observed.is_none(), "{observed:?}");
    }

    /// Clause 5.6.3.4 appends Scopes without overwriting: the observation is the
    /// Scope the Entity ends up in, and nothing when the append added none.
    #[test]
    fn a_no_overwrite_scope_append_is_observed_as_the_resulting_scope_only_when_it_changed() {
        let fragment = entity(&json!({SCOPE: [{"@value": "/Madrid"}]}));
        let before = entity(&json!({SCOPE: [{"@value": "/Berlin"}]}));
        let after = entity(&json!({SCOPE: [{"@value": "/Berlin"}, {"@value": "/Madrid"}]}));

        let observed = written_observation(entity_fragment(&fragment, OverwritePolicy::Preserve), &before, &after).expect("readable datasetIds");
        pretty_assertions::assert_eq!(observed_members(observed)[SCOPE], json!([{"@value": "/Berlin"}, {"@value": "/Madrid"}]));

        let unchanged = written_observation(entity_fragment(&fragment, OverwritePolicy::Preserve), &after, &after).expect("readable datasetIds");
        assert!(unchanged.is_none(), "{unchanged:?}");
    }

    /// Clause 4.5.7: the stored `instanceId` names an earlier instance, so the
    /// observation carries only the one the request supplied.
    #[test]
    fn the_observation_never_reuses_an_instance_id_an_earlier_write_left_behind() {
        let attribute = iri(TEMPERATURE);
        let stored = with(property(&json!(2)), INSTANCE_ID, &json!([{"@id": "urn:ngsi-ld:Instance:earlier"}]));
        let state = entity(&json!({TEMPERATURE: [stored]}));

        let without = written_observation(
            AddressedInstances::AttributeFragment {
                attribute: &attribute,
                dataset_id: None,
                fragment: &property(&json!(2)),
            },
            &state,
            &state,
        )
        .expect("readable datasetIds");
        pretty_assertions::assert_eq!(observed_members(without)[TEMPERATURE], json!([property(&json!(2))]));

        let supplied = with(property(&json!(2)), INSTANCE_ID, &json!([{"@id": "urn:ngsi-ld:Instance:new"}]));
        let with_own = written_observation(
            AddressedInstances::AttributeFragment {
                attribute: &attribute,
                dataset_id: None,
                fragment: &supplied,
            },
            &state,
            &state,
        )
        .expect("readable datasetIds");
        pretty_assertions::assert_eq!(observed_members(with_own)[TEMPERATURE], json!([supplied]));
    }

    /// Members NGSI-LD reserves are not attributes and are never observed as one.
    #[test]
    fn reserved_entity_members_are_not_observed_as_attributes() {
        let expires_at = "https://uri.etsi.org/ngsi-ld/expiresAt";
        let fragment = entity(&json!({expires_at: [{"@value": "2030-01-01T00:00:00Z"}]}));
        let state = entity(&json!({expires_at: [{"@value": "2030-01-01T00:00:00Z"}]}));

        let observed = written_observation(entity_fragment(&fragment, OverwritePolicy::Overwrite), &state, &state).expect("readable datasetIds");

        assert!(observed.is_none(), "{observed:?}");
    }

    #[test]
    fn an_unreadable_dataset_id_is_an_error() {
        let fragment = entity(&json!({TEMPERATURE: [with(property(&json!(1)), DATASET_ID, &dataset("not a uri"))]}));
        let state = entity(&json!({TEMPERATURE: [property(&json!(1))]}));

        assert!(written_observation(entity_fragment(&fragment, OverwritePolicy::Overwrite), &state, &state).is_err());
    }

    #[test]
    fn a_state_that_is_not_an_entity_node_observes_nothing() {
        let fragment = entity(&json!({NAME: [property(&json!("lobby"))]}));
        let observed = written_observation(entity_fragment(&fragment, OverwritePolicy::Overwrite), &Value::Null, &Value::Null).expect("readable datasetIds");
        assert!(observed.is_none());
    }
}
