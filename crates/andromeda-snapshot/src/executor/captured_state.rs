//! What a captured state keeps of what it holds (clause 5.16.1.4).
//!
//! Clause 5.16.1.4 freezes the result a client query (clause 5.7.2.4) would
//! have received. A stored state the query selected already holds only the
//! attributes its `attrs` and projection keep; the `datasetId` selection the
//! query's rendering would still apply is applied here, to the state itself,
//! so the state goes into the Snapshot's branch as stored rather than through
//! a compact rendering and back.

use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_query::dataset_selector::{DatasetFilter, DatasetSelector};
use std::sync::Arc;

/// Narrow a captured state to the instances `dataset_ids` selects when the
/// query names datasets (clause 4.5.5), decided as every execution that
/// narrows instances by `datasetId` decides it. An attribute left with no
/// instance is removed, and one every instance of which is selected is left
/// shared with the stored state.
pub(crate) fn narrow_captured(state: &mut EntityState, dataset_ids: Option<&[DatasetSelector]>) {
    let Some(selectors) = dataset_ids else {
        return;
    };
    let filter = DatasetFilter::from_selectors(selectors);
    state.attributes_mut().retain(|_, instances| {
        if instances.iter().all(|(dataset_id, _)| filter.selects(dataset_id)) {
            return true;
        }
        Arc::make_mut(instances).retain(|dataset_id, _| filter.selects(dataset_id))
    });
}

#[cfg(test)]
mod tests {
    use crate::executor::captured_state::narrow_captured;
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_query::dataset_selector::DatasetSelector;
    use serde_json::json;
    use std::sync::Arc;

    const SPEED: &str = "https://example.org/speed";
    const BRAND: &str = "https://example.org/brand";

    fn property(value: i64, dataset_id: Option<&str>) -> serde_json::Value {
        let mut instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]
        });
        if let Some(dataset_id) = dataset_id {
            instance["https://uri.etsi.org/ngsi-ld/datasetId"] = json!([{"@id": dataset_id}]);
        }
        instance
    }

    fn state(speed: &[serde_json::Value], brand: &[serde_json::Value]) -> EntityState {
        let mut node = json!({"@id": "urn:ngsi-ld:Vehicle:1", "@type": ["https://example.org/Vehicle"]});
        if !speed.is_empty() {
            node[SPEED] = json!(speed);
        }
        if !brand.is_empty() {
            node[BRAND] = json!(brand);
        }
        EntityState::from_slice(json!([node]).to_string().as_bytes()).expect("a stored state")
    }

    fn dataset_ids(state: &EntityState, attribute: &str) -> Vec<Option<String>> {
        state
            .attributes()
            .get(attribute)
            .map_or_default(|instances| instances.iter().map(|(dataset_id, _)| dataset_id.map(ToString::to_string)).collect())
    }

    fn dataset(id: &str) -> DatasetSelector {
        DatasetSelector::Id(id.parse().expect("a dataset identifier"))
    }

    /// Clause 4.5.5: a query naming datasets captures only their instances;
    /// `@none` selects the default instance.
    #[test]
    fn a_dataset_selection_keeps_only_the_selected_instances() {
        let mut captured = state(
            &[property(1, None), property(2, Some("urn:ds:a")), property(3, Some("urn:ds:b"))],
            &[property(4, Some("urn:ds:b"))],
        );
        narrow_captured(&mut captured, Some(&[DatasetSelector::Default, dataset("urn:ds:a")]));
        pretty_assertions::assert_eq!(dataset_ids(&captured, SPEED), vec![None, Some("urn:ds:a".to_owned())]);
        assert!(!captured.attributes().contains(BRAND), "brand held only an unselected instance");
    }

    /// An attribute every instance of which the selection keeps is not copied.
    #[test]
    fn an_attribute_kept_whole_stays_shared_with_the_stored_state() {
        let stored = state(&[property(1, Some("urn:ds:a"))], &[property(2, Some("urn:ds:b"))]);
        let mut captured = stored.clone();
        narrow_captured(&mut captured, Some(&[dataset("urn:ds:a")]));
        let shared = |state: &EntityState| Arc::clone(state.attributes().get_shared(SPEED).expect("speed"));
        assert!(Arc::ptr_eq(&shared(&stored), &shared(&captured)));
        assert!(!captured.attributes().contains(BRAND));
    }

    /// Without a dataset selection the state is captured as stored.
    #[test]
    fn a_state_without_a_dataset_selection_is_captured_as_stored() {
        let stored = state(&[property(1, None)], &[property(2, Some("urn:ds:b"))]);
        let mut captured = stored.clone();
        narrow_captured(&mut captured, None);
        pretty_assertions::assert_eq!(captured.to_vec().expect("encodes"), stored.to_vec().expect("encodes"));
    }
}
