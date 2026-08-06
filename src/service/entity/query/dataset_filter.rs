//! Narrowing an entity's attribute instances by `datasetId` (clause 4.5.5).

use andromeda_ngsi_ld_model::{attribute::instance::AttributeValue, entity::Entity};

/// The `datasetId` token that selects the instance carrying none, per clause
/// 4.5.5.
const DEFAULT_INSTANCE: &str = "@none";

/// Retain only the attribute instances whose `datasetId` the caller named.
///
/// An attribute left with no instance is removed entirely: clause 4.5.5 makes
/// `datasetId` an instance selector, and an attribute whose every instance was
/// filtered out is not part of the answer.
pub(super) fn filter_entity_by_dataset_id(entity: &mut Entity, dataset_ids: &[String]) {
    let mut keys_to_remove = Vec::new();
    for (key, attr_val) in entity.attrs.iter_mut() {
        match attr_val {
            AttributeValue::Single(attr) => {
                if !selected(attr.as_ref().dataset_id_string().as_deref(), dataset_ids) {
                    keys_to_remove.push(key.clone()); // clone: owned key to avoid borrowing entity.attrs during removal
                }
            }
            AttributeValue::Multiple(list) => {
                list.retain(|attr| selected(attr.dataset_id_string().as_deref(), dataset_ids));
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
fn selected(dataset_id: Option<&str>, wanted: &[String]) -> bool {
    wanted.iter().any(|candidate| match candidate.as_str() {
        DEFAULT_INSTANCE => dataset_id.is_none(),
        // `datasetId` is a URI and the parameter is unbounded text, so a
        // catch-all is the only way to close this match.
        named => dataset_id == Some(named),
    })
}

#[cfg(test)]
mod tests {
    use crate::service::entity::query::dataset_filter::selected;

    #[test]
    fn the_none_token_selects_the_instance_that_carries_no_dataset_id() {
        // Clause 4.5.5: `@none` names the default instance.
        let wanted = ["@none".to_string()];
        assert!(selected(None, &wanted));
        assert!(!selected(Some("urn:ngsi-ld:Dataset:1"), &wanted));
    }

    #[test]
    fn a_named_dataset_id_selects_only_the_instance_carrying_it() {
        let wanted = ["urn:ngsi-ld:Dataset:1".to_string()];
        assert!(selected(Some("urn:ngsi-ld:Dataset:1"), &wanted));
        assert!(!selected(Some("urn:ngsi-ld:Dataset:2"), &wanted));
        assert!(!selected(None, &wanted));
    }

    #[test]
    fn a_mixed_selection_admits_both_the_default_instance_and_the_named_one() {
        let wanted = ["@none".to_string(), "urn:ngsi-ld:Dataset:1".to_string()];
        assert!(selected(None, &wanted));
        assert!(selected(Some("urn:ngsi-ld:Dataset:1"), &wanted));
        assert!(!selected(Some("urn:ngsi-ld:Dataset:2"), &wanted));
    }
}
