//! What a captured stored state keeps of what it holds (clause 5.16.1.4).
//!
//! Clause 5.16.1.4 freezes the result a client query (clause 5.7.2.4) would
//! have received. A stored state the query selected already holds only the
//! attributes its `attrs` and projection keep; what the query's rendering
//! would still have dropped is applied here, to the state itself, so the state
//! goes into the Snapshot's branch as stored rather than through a compact
//! rendering and back.

use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Narrow a captured state to what the query's result holds at `captured_at`:
/// no attribute instance whose `expiresAt` has passed (clause 4.22), and only
/// the instances `dataset_ids` selects when the query names datasets (clause
/// 4.5.5). An attribute left with no instance is removed.
pub(crate) fn narrow_captured(state: &mut EntityState, captured_at: DateTime<Utc>, dataset_ids: Option<&[DatasetSelector]>) {
    state.remove_expired_instances(captured_at);
    if let Some(selectors) = dataset_ids {
        retain_selected_datasets(state, selectors);
    }
}

/// Keep only the attribute instances one of `selectors` selects. An attribute
/// every instance of which is selected is left shared with the stored state.
fn retain_selected_datasets(state: &mut EntityState, selectors: &[DatasetSelector]) {
    state.attributes_mut().retain(|_, instances| {
        if instances.iter().all(|(dataset_id, _)| selected(selectors, dataset_id)) {
            return true;
        }
        Arc::make_mut(instances).retain(|dataset_id, _| selected(selectors, dataset_id))
    });
}

/// Whether one of `selectors` selects the instance of `dataset_id`.
fn selected(selectors: &[DatasetSelector], dataset_id: Option<&Identifier>) -> bool {
    selectors.iter().any(|selector| selector.selects(dataset_id))
}

#[cfg(test)]
mod tests {
    use crate::executor::captured_state::narrow_captured;
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use andromeda_ngsi_ld_query::dataset::DatasetSelector;
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use std::sync::Arc;

    const SPEED: &str = "https://example.org/speed";
    const BRAND: &str = "https://example.org/brand";

    fn property(value: i64, dataset_id: Option<&str>, expires_at: Option<&str>) -> serde_json::Value {
        let mut instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]
        });
        if let Some(dataset_id) = dataset_id {
            instance["https://uri.etsi.org/ngsi-ld/datasetId"] = json!([{"@id": dataset_id}]);
        }
        if let Some(expires_at) = expires_at {
            instance["https://uri.etsi.org/ngsi-ld/expiresAt"] = json!([{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": expires_at}]);
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

    fn instant(text: &str) -> DateTime<Utc> {
        text.parse::<NgsiLdDateTime>().expect("an instant").get()
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

    /// Clause 4.22: an instance whose `expiresAt` passed before the capture is
    /// not captured, and an attribute left with none is gone.
    #[test]
    fn an_instance_expired_at_capture_time_is_not_captured() {
        let mut captured = state(
            &[property(1, None, Some("2025-01-01T00:00:00Z"))],
            &[property(2, None, Some("2027-01-01T00:00:00Z"))],
        );
        narrow_captured(&mut captured, instant("2026-01-01T00:00:00Z"), None);
        assert!(!captured.attributes().contains(SPEED));
        assert!(captured.attributes().contains(BRAND));
    }

    /// Clause 4.5.5: a query naming datasets captures only their instances;
    /// `@none` selects the default instance.
    #[test]
    fn a_dataset_selection_keeps_only_the_selected_instances() {
        let mut captured = state(
            &[
                property(1, None, None),
                property(2, Some("urn:ds:a"), None),
                property(3, Some("urn:ds:b"), None),
            ],
            &[property(4, Some("urn:ds:b"), None)],
        );
        narrow_captured(
            &mut captured,
            instant("2026-01-01T00:00:00Z"),
            Some(&[DatasetSelector::Default, dataset("urn:ds:a")]),
        );
        pretty_assertions::assert_eq!(dataset_ids(&captured, SPEED), vec![None, Some("urn:ds:a".to_owned())]);
        assert!(!captured.attributes().contains(BRAND), "brand held only an unselected instance");
    }

    /// An attribute every instance of which the selection keeps is not copied.
    #[test]
    fn an_attribute_kept_whole_stays_shared_with_the_stored_state() {
        let stored = state(&[property(1, Some("urn:ds:a"), None)], &[property(2, Some("urn:ds:b"), None)]);
        let mut captured = stored.clone();
        narrow_captured(&mut captured, instant("2026-01-01T00:00:00Z"), Some(&[dataset("urn:ds:a")]));
        let shared = |state: &EntityState| Arc::clone(state.attributes().get_shared(SPEED).expect("speed"));
        assert!(Arc::ptr_eq(&shared(&stored), &shared(&captured)));
        assert!(!captured.attributes().contains(BRAND));
    }

    /// Without a dataset selection or an expired instance the state is
    /// captured as stored.
    #[test]
    fn a_state_with_nothing_to_drop_is_captured_as_stored() {
        let stored = state(&[property(1, None, None)], &[property(2, Some("urn:ds:b"), None)]);
        let mut captured = stored.clone();
        narrow_captured(&mut captured, instant("2026-01-01T00:00:00Z"), None);
        pretty_assertions::assert_eq!(captured.to_vec().expect("encodes"), stored.to_vec().expect("encodes"));
    }
}
