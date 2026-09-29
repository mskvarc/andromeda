//! Narrowing an entity's attribute instances by `datasetId` (clause 4.5.5).

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{attribute::instance::AttributeValue, entity::Entity};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;

/// Retain only the attribute instances whose `datasetId` the caller named.
///
/// An attribute left with no instance is removed entirely: clause 4.5.5 makes
/// `datasetId` an instance selector, and an attribute whose every instance was
/// filtered out is not part of the answer.
pub(super) fn filter_entity_by_dataset_id(entity: &mut Entity, dataset_ids: &[DatasetSelector]) {
    let mut keys_to_remove = Vec::new();
    for (key, attr_val) in &mut entity.attrs {
        match attr_val {
            AttributeValue::Single(attr) => {
                if !selected(attr.as_ref().dataset_id(), dataset_ids) {
                    keys_to_remove.push(key.clone()); // clone: owned key to avoid borrowing entity.attrs during removal
                }
            }
            AttributeValue::Multiple(list) => {
                list.retain(|attr| selected(attr.dataset_id(), dataset_ids));
                if list.is_empty() {
                    keys_to_remove.push(key.clone()); // clone: owned key to avoid borrowing entity.attrs during removal
                } else if list.len() == 1 {
                    *attr_val = AttributeValue::Single(Box::new(list.remove(0)));
                }
            }
            AttributeValue::Raw(_) => {}
        }
    }
    for key in keys_to_remove {
        entity.attrs.shift_remove(&key);
    }
}

/// Whether one instance's `datasetId` is among those the caller named.
fn selected(dataset_id: Option<&Identifier>, wanted: &[DatasetSelector]) -> bool {
    wanted.iter().any(|candidate| match candidate {
        DatasetSelector::Default => dataset_id.is_none(),
        DatasetSelector::Id(uri) => dataset_id == Some(uri),
    })
}

#[cfg(test)]
mod tests {
    use crate::query::dataset_filter::selected;
    use andromeda_ngsi_ld_query::dataset::DatasetSelector;

    fn selectors(values: &[&str]) -> Vec<DatasetSelector> {
        values
            .iter()
            .map(|value| value.parse::<DatasetSelector>().expect("valid dataset selector"))
            .collect()
    }

    #[test]
    fn the_none_token_selects_the_instance_that_carries_no_dataset_id() {
        // Clause 4.5.5: `@none` names the default instance.
        let wanted = selectors(&["@none"]);
        assert!(selected(None, &wanted));
        let dataset_id = "urn:ngsi-ld:Dataset:1".parse().expect("valid dataset URI");
        assert!(!selected(Some(&dataset_id), &wanted));
    }

    #[test]
    fn a_named_dataset_id_selects_only_the_instance_carrying_it() {
        let wanted = selectors(&["urn:ngsi-ld:Dataset:1"]);
        let selected_id = "urn:ngsi-ld:Dataset:1".parse().expect("valid dataset URI");
        let other_id = "urn:ngsi-ld:Dataset:2".parse().expect("valid dataset URI");
        assert!(selected(Some(&selected_id), &wanted));
        assert!(!selected(Some(&other_id), &wanted));
        assert!(!selected(None, &wanted));
    }

    #[test]
    fn a_mixed_selection_admits_both_the_default_instance_and_the_named_one() {
        let wanted = selectors(&["@none", "urn:ngsi-ld:Dataset:1"]);
        assert!(selected(None, &wanted));
        let selected_id = "urn:ngsi-ld:Dataset:1".parse().expect("valid dataset URI");
        let other_id = "urn:ngsi-ld:Dataset:2".parse().expect("valid dataset URI");
        assert!(selected(Some(&selected_id), &wanted));
        assert!(!selected(Some(&other_id), &wanted));
    }
}
