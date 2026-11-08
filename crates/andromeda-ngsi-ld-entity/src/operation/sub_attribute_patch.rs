//! Applying the written sub-attributes of an instance (clause 4.5.2) to the
//! sub-attributes it holds.
//!
//! A sub-attribute is an attribute of its instance (clause 4.5.2.2), so its
//! instances are keyed by `datasetId` exactly as an attribute's are (clause
//! 4.5.5.1), and a written sub-attribute instance lands only on the held
//! instance with its `datasetId`, the default one when it names none, or is
//! added beside the others (clauses 5.5.8, 5.5.12). The NGSI-LD Null has the
//! two spellings clause 4.5.0 gives it: the bare Null a non-reified member
//! takes removes the sub-attribute with every instance, while an instance
//! whose value member is its family's deletion encoding (clauses 4.5.2,
//! 4.5.3, 4.5.18, 4.5.21 and 4.5.22) removes only the instance with its
//! `datasetId` (clause 5.6.5.4).

use crate::{
    attribute_instances::AttributeInstances,
    attributes::Attributes,
    document::{
        attribute_member::AttributeMember,
        instance_document::InstanceDocument,
        instance_identities::admit_distinct_identities,
        written_value::ValueSlot,
    },
    operation::{
        attribute_write::apply_instances,
        instance_patch::{InstancePatchDepth, new_instance},
        mutation_error::MutationError,
    },
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use indexmap::IndexMap;

/// The sub-attributes a new instance takes from the written ones: deletions
/// have nothing to delete and are left out.
///
/// # Errors
///
/// Returns the refusal of a written sub-attribute instance that cannot
/// become state, or of two that name one instance.
pub(crate) fn new_sub_attributes(written: &IndexMap<AttributeName, AttributeMember>) -> Result<Attributes, MutationError> {
    let mut attributes = Attributes::new();
    for (name, member) in written {
        match member {
            AttributeMember::Remove => {}
            AttributeMember::Instances(documents) => {
                admit_distinct_identities(name, documents)?;
                if let Some(instances) = settled_instances(name, documents)? {
                    attributes.insert(name.clone(), instances);
                }
            }
        }
    }
    Ok(attributes)
}

/// Apply written sub-attributes to the held ones, in place.
///
/// Each written instance lands as an attribute's does (clauses 5.5.8,
/// 5.5.12): on the held instance with its `datasetId` under `depth`, or
/// added, and a deletion encoding removes only the instance with its
/// `datasetId`. A sub-attribute set to the bare NGSI-LD Null is removed with
/// every instance.
///
/// No default `observedAt` is stamped: Table 6.5.3.4-1 gives it to the
/// merged Attributes only (clause 5.6.17.4).
///
/// # Errors
///
/// Returns the refusal of a written sub-attribute instance that cannot
/// become state, or of two that name one instance.
pub(crate) fn patch_sub_attributes(
    held: &mut Attributes,
    written: &IndexMap<AttributeName, AttributeMember>,
    depth: InstancePatchDepth,
) -> Result<(), MutationError> {
    written.iter().try_for_each(|(name, member)| match member {
        AttributeMember::Remove => {
            held.remove(name);
            Ok(())
        }
        AttributeMember::Instances(documents) => {
            admit_distinct_identities(name, documents)?;
            apply_instances(held, name, documents, depth, None)
        }
    })
}

/// The instances a written member makes on its own, or `None` when it makes
/// none.
fn settled_instances(name: &AttributeName, documents: &[InstanceDocument]) -> Result<Option<AttributeInstances>, MutationError> {
    let instances = documents
        .iter()
        .filter(|document| !matches!(document.value(), ValueSlot::Remove(_)))
        .map(|document| new_instance(name, document, None).map(|instance| (document.dataset_id().cloned(), instance)))
        .collect::<Result<Vec<_>, MutationError>>()?;
    AttributeInstances::try_from_instances(instances).map_err(|repeated| EntityRefusal::repeated_instance(name, repeated).into())
}

#[cfg(test)]
mod tests {
    use crate::{
        attributes::Attributes,
        document::{attribute_member::AttributeMember, instance_document::InstanceDocument},
        instance::Instance,
        operation::{
            instance_patch::InstancePatchDepth,
            mutation_error::MutationError,
            speed_entity::{ACCURACY, dataset, date_time, instance_rows, instances, name, number, property, property_deletion, property_document, row},
            sub_attribute_patch::{new_sub_attributes, patch_sub_attributes},
        },
        refusal::EntityRefusal,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time::date_time::NgsiLdDateTime};
    use indexmap::IndexMap;

    const A: &str = "urn:ngsi-ld:Dataset:A";
    const B: &str = "urn:ngsi-ld:Dataset:B";
    const C: &str = "urn:ngsi-ld:Dataset:C";
    const OBSERVED: &str = "2020-01-01T00:00:00Z";

    /// A held Property instance holding `value`, observed at [`OBSERVED`].
    fn observed(value: i64) -> Instance {
        let mut instance = property(number(value));
        instance.set_observed_at(Some(date_time(OBSERVED)));
        instance
    }

    /// The held `accuracy` sub-attribute with the given instances.
    fn accuracy(held: Vec<(Option<&str>, Instance)>) -> Attributes {
        [(name(ACCURACY), instances(held))].into_iter().collect()
    }

    /// The written `accuracy` sub-attribute with the given instances.
    fn written(documents: Vec<InstanceDocument>) -> IndexMap<AttributeName, AttributeMember> {
        [(name(ACCURACY), AttributeMember::Instances(documents))].into_iter().collect()
    }

    /// The `observedAt` of the held `accuracy` instance with `dataset_id`.
    fn observed_at(attributes: &Attributes, dataset_id: Option<&str>) -> Option<NgsiLdDateTime> {
        let dataset_id = dataset_id.map(dataset);
        attributes
            .get(ACCURACY)
            .and_then(|held| held.get(dataset_id.as_ref()))
            .and_then(Instance::observed_at)
    }

    /// Land `documents` on `held` under `depth`, and expect it to apply.
    fn landed(mut held: Attributes, documents: Vec<InstanceDocument>, depth: InstancePatchDepth) -> Attributes {
        patch_sub_attributes(&mut held, &written(documents), depth).expect("the sub-attributes land");
        held
    }

    /// Clause 5.5.12: a member "is only updated if the datasetId is the same,
    /// otherwise the member of the Fragment is added as a new instance";
    /// clause 4.5.5.1 leaves the instance of another `datasetId` untouched.
    #[test]
    fn a_merged_sub_attribute_with_another_dataset_id_is_added_beside_the_held_one() {
        let held = landed(
            accuracy(vec![(Some(A), property(number(1)))]),
            vec![property_document(number(2), Some(B))],
            InstancePatchDepth::DeepMerge,
        );
        pretty_assertions::assert_eq!(instance_rows(&held, ACCURACY), vec![row(Some(A), 1), row(Some(B), 2)]);
    }

    /// Clause 5.5.12: "If no datasetId is present, the default Attribute
    /// instance is targeted and merged if present and otherwise added."
    #[test]
    fn a_merged_default_sub_attribute_is_added_beside_a_held_dataset_instance() {
        let held = landed(
            accuracy(vec![(Some(A), property(number(1)))]),
            vec![property_document(number(2), None)],
            InstancePatchDepth::DeepMerge,
        );
        pretty_assertions::assert_eq!(instance_rows(&held, ACCURACY), vec![row(Some(A), 1), row(None, 2)]);
    }

    /// Clause 5.5.12 merges the instance with the written `datasetId`, and
    /// clause 4.5.5.1 leaves the others untouched.
    #[test]
    fn a_merged_dataset_instance_merges_only_its_own_instance() {
        let held = landed(
            accuracy(vec![(Some(A), observed(1)), (Some(B), property(number(5)))]),
            vec![property_document(number(2), Some(A))],
            InstancePatchDepth::DeepMerge,
        );
        pretty_assertions::assert_eq!(instance_rows(&held, ACCURACY), vec![row(Some(A), 2), row(Some(B), 5)]);
        pretty_assertions::assert_eq!(observed_at(&held, Some(A)), Some(date_time(OBSERVED)), "merged, not replaced");
    }

    /// Clause 5.5.12: a written instance naming no `datasetId` merges into the
    /// default instance alone.
    #[test]
    fn a_merged_default_instance_merges_only_the_default() {
        let held = landed(
            accuracy(vec![(None, observed(1)), (Some(A), property(number(5)))]),
            vec![property_document(number(2), None)],
            InstancePatchDepth::DeepMerge,
        );
        pretty_assertions::assert_eq!(instance_rows(&held, ACCURACY), vec![row(None, 2), row(Some(A), 5)]);
        pretty_assertions::assert_eq!(observed_at(&held, None), Some(date_time(OBSERVED)), "merged, not replaced");
    }

    /// Clauses 5.5.12 and 4.5.5.1 apply to each written instance in turn.
    #[test]
    fn several_written_sub_attribute_instances_land_one_by_one() {
        let held = landed(
            accuracy(vec![(Some(A), observed(1)), (Some(B), property(number(5)))]),
            vec![property_document(number(2), Some(A)), property_document(number(9), Some(C))],
            InstancePatchDepth::DeepMerge,
        );
        pretty_assertions::assert_eq!(instance_rows(&held, ACCURACY), vec![row(Some(A), 2), row(Some(B), 5), row(Some(C), 9)]);
        pretty_assertions::assert_eq!(observed_at(&held, Some(A)), Some(date_time(OBSERVED)), "merged, not replaced");
    }

    /// Clause 5.5.12 hands the deletion of "a specific Attribute instance
    /// with a datasetId" to clause 5.6.5, which removes only that instance
    /// (clause 5.6.5.4).
    #[test]
    fn a_sub_attribute_deletion_with_a_dataset_id_removes_only_that_instance() {
        let held = landed(
            accuracy(vec![(None, property(number(1))), (Some(A), property(number(5)))]),
            vec![property_deletion(Some(A))],
            InstancePatchDepth::DeepMerge,
        );
        pretty_assertions::assert_eq!(instance_rows(&held, ACCURACY), vec![row(None, 1)]);
    }

    /// Clause 5.6.5.4: with no `datasetId`, "remove the default target
    /// Attribute instance".
    #[test]
    fn a_sub_attribute_deletion_without_a_dataset_id_removes_only_the_default() {
        let held = landed(
            accuracy(vec![(None, property(number(1))), (Some(A), property(number(5)))]),
            vec![property_deletion(None)],
            InstancePatchDepth::DeepMerge,
        );
        pretty_assertions::assert_eq!(instance_rows(&held, ACCURACY), vec![row(Some(A), 5)]);
    }

    /// Clause 5.6.5.4: the sub-attribute goes with its last instance.
    #[test]
    fn a_deletion_of_the_last_sub_attribute_instance_removes_the_sub_attribute() {
        let held = landed(
            accuracy(vec![(Some(A), property(number(1)))]),
            vec![property_deletion(Some(A))],
            InstancePatchDepth::DeepMerge,
        );
        assert!(held.get(ACCURACY).is_none(), "{held:?}");
    }

    /// Clause 5.5.12 removes only a member "contained by the target".
    #[test]
    fn a_deletion_of_an_absent_sub_attribute_instance_changes_nothing() {
        let held = landed(
            accuracy(vec![(Some(A), property(number(1)))]),
            vec![property_deletion(Some(B))],
            InstancePatchDepth::DeepMerge,
        );
        pretty_assertions::assert_eq!(instance_rows(&held, ACCURACY), vec![row(Some(A), 1)]);
    }

    /// Clauses 5.5.12 and 5.6.17.4: a deletion sharing the member with an
    /// update deletes its instance and the update still lands.
    #[test]
    fn a_mixed_sub_attribute_array_deletes_one_instance_and_merges_another() {
        let held = landed(
            accuracy(vec![(Some(A), property(number(1))), (Some(B), property(number(5)))]),
            vec![property_deletion(Some(A)), property_document(number(6), Some(B))],
            InstancePatchDepth::DeepMerge,
        );
        pretty_assertions::assert_eq!(instance_rows(&held, ACCURACY), vec![row(Some(B), 6)]);
    }

    /// Clause 5.5.12: a member set to the NGSI-LD Null is removed with every
    /// instance.
    #[test]
    fn a_sub_attribute_set_to_the_null_is_deleted() {
        let mut held = accuracy(vec![(None, property(number(1))), (Some(A), property(number(5)))]);
        let written: IndexMap<_, _> = [(name(ACCURACY), AttributeMember::Remove)].into_iter().collect();
        patch_sub_attributes(&mut held, &written, InstancePatchDepth::DeepMerge).expect("the deletion applies");
        assert!(held.is_empty());
    }

    /// Clause 5.5.8: a member "is only replaced if the datasetId is the same,
    /// otherwise the member of the Fragment is added as a new instance".
    #[test]
    fn a_partially_updated_sub_attribute_with_another_dataset_id_is_added() {
        let held = landed(
            accuracy(vec![(Some(A), property(number(1)))]),
            vec![property_document(number(2), Some(B))],
            InstancePatchDepth::ReplaceInstance,
        );
        pretty_assertions::assert_eq!(instance_rows(&held, ACCURACY), vec![row(Some(A), 1), row(Some(B), 2)]);
    }

    /// Clause 5.5.8: the instance with the written `datasetId` is replaced
    /// whole, and the others stay.
    #[test]
    fn a_partially_updated_sub_attribute_instance_is_replaced_whole() {
        let held = landed(
            accuracy(vec![(Some(A), observed(1)), (Some(B), property(number(5)))]),
            vec![property_document(number(2), Some(A))],
            InstancePatchDepth::ReplaceInstance,
        );
        pretty_assertions::assert_eq!(instance_rows(&held, ACCURACY), vec![row(Some(A), 2), row(Some(B), 5)]);
        pretty_assertions::assert_eq!(observed_at(&held, Some(A)), None, "replaced, not merged");
    }

    /// Clause 5.5.12: a written sub-attribute instance merges into the held
    /// one, keeping what it omits.
    #[test]
    fn a_merged_sub_attribute_keeps_the_members_it_omits() {
        let held = landed(
            accuracy(vec![(None, observed(1))]),
            vec![property_document(number(2), None)],
            InstancePatchDepth::DeepMerge,
        );
        pretty_assertions::assert_eq!(instance_rows(&held, ACCURACY), vec![row(None, 2)]);
        pretty_assertions::assert_eq!(observed_at(&held, None), Some(date_time(OBSERVED)));
    }

    /// Clause 5.5.8: a written sub-attribute instance replaces the held one
    /// whole.
    #[test]
    fn a_replaced_sub_attribute_drops_the_members_it_omits() {
        let held = landed(
            accuracy(vec![(None, observed(1))]),
            vec![property_document(number(2), None)],
            InstancePatchDepth::ReplaceInstance,
        );
        pretty_assertions::assert_eq!(observed_at(&held, None), None);
    }

    /// Clause 4.5.5.1: two written instances of one sub-attribute naming one
    /// instance are refused alike whether the sub-attributes land on held ones
    /// or make new ones, a deletion among them included.
    #[test]
    fn a_repeated_sub_attribute_instance_is_refused_by_both_landings() {
        let cases = [
            (
                vec![property_document(number(1), None), property_document(number(2), None)],
                "the default instance twice",
            ),
            (
                vec![property_document(number(1), Some(A)), property_document(number(2), Some(A))],
                "one datasetId twice",
            ),
            (
                vec![property_deletion(Some(A)), property_document(number(2), Some(A))],
                "a deletion and a value of one datasetId",
            ),
        ];
        for (documents, case) in cases {
            let written = written(documents);
            let mut held = accuracy(vec![(Some(A), observed(7))]);
            let refusals = [
                new_sub_attributes(&written).expect_err("a repeated instance makes no sub-attributes"),
                patch_sub_attributes(&mut held, &written, InstancePatchDepth::DeepMerge).expect_err("a repeated instance lands on nothing"),
                patch_sub_attributes(&mut held, &written, InstancePatchDepth::ReplaceInstance).expect_err("a repeated instance lands on nothing"),
            ];
            for refused in refusals {
                assert!(
                    matches!(
                        refused,
                        MutationError::Refused(ref refusal)
                            if matches!(**refusal, EntityRefusal::MultipleDefaultInstances { .. } | EntityRefusal::RepeatedInstance { .. })
                    ),
                    "{case}: {refused:?}"
                );
            }
            pretty_assertions::assert_eq!(observed_at(&held, Some(A)), Some(date_time(OBSERVED)), "{case}: the held instance is untouched");
        }
    }
}
