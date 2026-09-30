//! Narrowing an entity's attribute instances by `datasetId` (clause 4.5.5).

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
                if !dataset_ids.iter().any(|selector| selector.selects(attr.as_ref().dataset_id())) {
                    keys_to_remove.push(key.clone()); // clone: owned key to avoid borrowing entity.attrs during removal
                }
            }
            AttributeValue::Multiple(list) => {
                list.retain(|attr| dataset_ids.iter().any(|selector| selector.selects(attr.dataset_id())));
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
