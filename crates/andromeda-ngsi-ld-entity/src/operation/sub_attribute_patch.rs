//! Applying the written sub-attributes of an instance (clause 4.5.2) to the
//! sub-attributes it holds.
//!
//! A sub-attribute set to the NGSI-LD Null is deleted whole, in either of the
//! spellings clause 4.5.0 gives it: the bare Null a non-reified member takes,
//! or one instance whose value member is its family's deletion encoding
//! (clauses 4.5.2, 4.5.3, 4.5.18, 4.5.21 and 4.5.22 each make that "a deletion
//! of the Property"). Clause 4.5.0 admits exactly one Null where an array is
//! required, so a member of several instances is data even when one of them
//! is a deletion.

use crate::{
    attribute_instances::{AttributeInstances, InstanceRemoval, RepeatedInstance},
    attributes::Attributes,
    document::{attribute_member::AttributeMember, instance_document::InstanceDocument, written_value::ValueSlot},
    operation::{
        instance_patch::{InstancePatchDepth, new_instance, patch_instance},
        mutation_error::MutationError,
    },
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use indexmap::IndexMap;

/// How a written sub-attribute meets the one an instance holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SubAttributeDepth {
    /// The written member replaces the held one wholesale (clause 5.5.8).
    Replace,
    /// A single written instance merges into a single held one (clause
    /// 5.5.12); any other shape is replaced.
    Merge,
}

/// The sub-attributes a new instance takes from the written ones: deletions
/// have nothing to delete and are left out.
///
/// # Errors
///
/// Returns the refusal of a written sub-attribute instance that cannot
/// become state, or of two that share a `datasetId`.
pub(crate) fn new_sub_attributes(written: &IndexMap<AttributeName, AttributeMember>) -> Result<Attributes, MutationError> {
    let mut attributes = Attributes::new();
    for (name, member) in written {
        match member {
            AttributeMember::Remove => {}
            AttributeMember::Instances(documents) => {
                if let Some(instances) = settled_instances(name, documents)? {
                    // clone: the stored instance owns the sub-attribute name.
                    attributes.insert(name.clone(), instances);
                }
            }
        }
    }
    Ok(attributes)
}

/// Apply written sub-attributes to the held ones, in place.
///
/// # Errors
///
/// As [`new_sub_attributes`].
pub(crate) fn patch_sub_attributes(
    held: &mut Attributes,
    written: &IndexMap<AttributeName, AttributeMember>,
    depth: SubAttributeDepth,
) -> Result<(), MutationError> {
    for (name, member) in written {
        let documents = match member {
            AttributeMember::Remove => {
                held.remove(name);
                continue;
            }
            AttributeMember::Instances(documents) => documents,
        };
        match documents.as_slice() {
            [single] if matches!(single.value(), ValueSlot::Remove(_)) => {
                held.remove(name);
            }
            [single] if depth == SubAttributeDepth::Merge && held.get(name).is_some_and(|instances| instances.len() == 1) => {
                merge_single(held, name, single)?;
            }
            documents => match settled_instances(name, documents)? {
                // clone: the stored instance owns the sub-attribute name.
                Some(instances) => {
                    held.insert(name.clone(), instances);
                }
                None => {
                    held.remove(name);
                }
            },
        }
    }
    Ok(())
}

/// Merge one written sub-attribute instance into the one instance held.
///
/// The two are merged whatever their `datasetId`s, and the written
/// `datasetId`, when there is one, becomes the merged instance's.
fn merge_single(held: &mut Attributes, name: &AttributeName, document: &InstanceDocument) -> Result<(), MutationError> {
    let Some(instances) = held.get_mut(name) else {
        return Ok(());
    };
    let Some(held_key) = instances.iter().next().map(|(dataset_id, _)| dataset_id.cloned()) else {
        return Ok(());
    };
    if let Some(instance) = instances.get_mut(held_key.as_ref()) {
        patch_instance(name, instance, document, InstancePatchDepth::DeepMerge, None)?;
    }
    if let Some(written_key) = document.dataset_id()
        && Some(written_key) != held_key.as_ref()
    {
        match instances.remove(held_key.as_ref()) {
            InstanceRemoval::Removed(instance) | InstanceRemoval::Emptied(instance) => {
                // clone: the instance is keyed by its own copy of the written identity.
                instances.insert(Some(written_key.clone()), instance);
            }
            InstanceRemoval::Absent => {}
        }
    }
    Ok(())
}

/// The instances a written member makes on its own, or `None` when it makes
/// none.
fn settled_instances(name: &AttributeName, documents: &[InstanceDocument]) -> Result<Option<AttributeInstances>, MutationError> {
    let instances = documents
        .iter()
        .filter(|document| !matches!(document.value(), ValueSlot::Remove(_)))
        .map(|document| new_instance(name, document, None).map(|instance| (document.dataset_id().cloned(), instance))) // clone: the instance is keyed by its own copy of the written identity.
        .collect::<Result<Vec<_>, MutationError>>()?;
    AttributeInstances::try_from_instances(instances).map_err(|repeated| repeated_instance(name, repeated))
}

/// The refusal of two written instances of one attribute sharing an identity
/// (clause 4.5.5).
pub(crate) fn repeated_instance(name: &AttributeName, repeated: RepeatedInstance) -> MutationError {
    // clone: the refusal outlives the borrowed request.
    let attribute = name.clone();
    match repeated {
        RepeatedInstance::Default => EntityRefusal::MultipleDefaultInstances { attribute },
        RepeatedInstance::Dataset(dataset_id) => EntityRefusal::RepeatedInstance { attribute, dataset_id },
    }
    .into()
}

#[cfg(test)]
mod tests {
    use crate::{
        attributes::Attributes,
        document::{attribute_member::AttributeMember, instance_document::InstanceDocument, slot::Slot},
        operation::{
            mutation_error::MutationError,
            speed_entity::{date_time, instances, integer_of, name, number, property, property_document},
            sub_attribute_patch::{SubAttributeDepth, new_sub_attributes, patch_sub_attributes},
        },
        refusal::EntityRefusal,
    };
    use indexmap::IndexMap;

    const ACCURACY: &str = "https://example.org/accuracy";

    fn held() -> Attributes {
        let mut observed = property(number(1));
        observed.set_observed_at(Some(date_time("2020-01-01T00:00:00Z")));
        [(name(ACCURACY), instances(vec![(None, observed)]))].into_iter().collect()
    }

    /// Clause 5.5.12: a single written sub-attribute instance merges into the
    /// single held one, keeping what it omits.
    #[test]
    fn a_merged_sub_attribute_keeps_the_members_it_omits() {
        let mut attributes = held();
        let written: IndexMap<_, _> = [(name(ACCURACY), AttributeMember::Instances(vec![property_document(number(2), None)]))]
            .into_iter()
            .collect();
        patch_sub_attributes(&mut attributes, &written, SubAttributeDepth::Merge).expect("the merge applies");
        let instance = attributes.get(ACCURACY).and_then(|held| held.get(None)).expect("the sub-attribute");
        pretty_assertions::assert_eq!(integer_of(instance), Some(2));
        pretty_assertions::assert_eq!(instance.observed_at(), Some(&date_time("2020-01-01T00:00:00Z")));
    }

    /// Clause 5.5.8: a written sub-attribute replaces the held one wholesale.
    #[test]
    fn a_replaced_sub_attribute_drops_the_members_it_omits() {
        let mut attributes = held();
        let written: IndexMap<_, _> = [(name(ACCURACY), AttributeMember::Instances(vec![property_document(number(2), None)]))]
            .into_iter()
            .collect();
        patch_sub_attributes(&mut attributes, &written, SubAttributeDepth::Replace).expect("the patch applies");
        let instance = attributes.get(ACCURACY).and_then(|held| held.get(None)).expect("the sub-attribute");
        pretty_assertions::assert_eq!(instance.observed_at(), None);
    }

    /// A member-level NGSI-LD Null deletes the sub-attribute.
    #[test]
    fn a_sub_attribute_set_to_the_null_is_deleted() {
        let mut attributes = held();
        let written: IndexMap<_, _> = [(name(ACCURACY), AttributeMember::Remove)].into_iter().collect();
        patch_sub_attributes(&mut attributes, &written, SubAttributeDepth::Merge).expect("the deletion applies");
        assert!(attributes.is_empty());
    }

    /// Clause 4.5.5: two written default instances of one sub-attribute are
    /// refused.
    #[test]
    fn two_default_instances_of_a_sub_attribute_are_refused() {
        let mut observed = property_document(number(1), None);
        observed.set_observed_at(Some(Slot::Set(date_time("2020-01-01T00:00:00Z"))));
        let written: IndexMap<_, _> = [(
            name(ACCURACY),
            AttributeMember::Instances(vec![
                observed,
                InstanceDocument::builder().value(property_document(number(2), None).value().clone()).build(),
            ]),
        )]
        .into_iter()
        .collect();
        let refused = new_sub_attributes(&written).expect_err("a repeated default instance");
        assert!(
            matches!(refused, MutationError::Refused(ref refusal) if matches!(**refusal, EntityRefusal::MultipleDefaultInstances { .. })),
            "{refused:?}"
        );
    }
}
