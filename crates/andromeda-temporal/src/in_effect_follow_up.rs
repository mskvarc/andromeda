//! The datasets a split temporal query asks its sources about one by one for
//! the instance in effect when its window opens (clauses 4.5.5 and 5.7.4.4).
//!
//! Table 6.18.3.2-1 applies `lastN` "per Attribute, per Entity", so a source
//! answering the in-effect probe that way returns the latest instance of one
//! dataset of each attribute, while the instance in effect is decided per
//! stream, attribute and `datasetId` alike. The `datasetId` filter runs before
//! the `lastN` cut (clause 5.7.4.4, step S8 ahead of the instance selection),
//! so a probe naming one dataset reads that dataset's latest instance. A
//! dataset is asked about when some part holds an instance of it for an
//! attribute a source answered the probe for with another dataset only. A
//! dataset no part holds, and that is not a source's latest, stays unseen.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::temporal::temporal_document::TemporalDocument;
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use std::collections::{HashMap, HashSet};

/// One attribute of one entity.
type AttributeOf<'a> = (&'a Identifier, &'a AttributeName);

/// The datasets of the attributes of every entity in `parts`.
type HeldDatasets<'a> = HashMap<AttributeOf<'a>, HashSet<Option<&'a Identifier>>>;

/// The datasets to probe one by one, given what each source answered the
/// probes across every dataset with (`probed`, one list per source) and every
/// part of the contributed entities, local and answered (`parts`).
#[must_use]
pub fn follow_up_datasets<'a>(probed: &[Vec<&'a TemporalDocument>], parts: impl IntoIterator<Item = &'a TemporalDocument>) -> Vec<DatasetSelector> {
    let mut held = HeldDatasets::new();
    for part in parts.into_iter().chain(probed.iter().flatten().copied()) {
        collect_datasets(part, &mut held);
    }
    let mut missing: Vec<DatasetSelector> = Vec::new();
    for answers in probed {
        let mut answered = HeldDatasets::new();
        for answer in answers {
            collect_datasets(answer, &mut answered);
        }
        for (attribute_of, datasets) in &answered {
            let unanswered = held.get(attribute_of).into_iter().flatten().filter(|dataset| !datasets.contains(*dataset));
            for dataset in unanswered {
                let selector = dataset.map_or(DatasetSelector::Default, |dataset| DatasetSelector::Id(dataset.clone())); // clone: the probe owns the dataset it names
                if !missing.contains(&selector) {
                    missing.push(selector);
                }
            }
        }
    }
    missing
}

/// Add the datasets every attribute instance of `part` is written under; the
/// Scope has no dataset (clause 4.5.4).
fn collect_datasets<'a>(part: &'a TemporalDocument, held: &mut HeldDatasets<'a>) {
    let Some(entity_id) = part.id() else {
        return;
    };
    for (attribute, member) in part.entity().attributes() {
        if attribute.as_str() == expanded_properties::SCOPE.as_str() {
            continue;
        }
        held.entry((entity_id, attribute))
            .or_default()
            .extend(member.instances().iter().map(|instance| instance.dataset_id()));
    }
}

#[cfg(test)]
mod tests {
    use crate::{answered_fixture::answered, in_effect_follow_up::follow_up_datasets};
    use andromeda_ngsi_ld_query::dataset::DatasetSelector;
    use serde_json::json;

    /// Clause 5.7.4.4 with Table 6.18.3.2-1: a source answering the probe per
    /// Attribute returned `d2` alone, while the local part holds `d1`, so `d1`
    /// is asked about on its own; the dataset the source answered is not.
    #[tokio::test]
    async fn a_dataset_another_part_holds_is_asked_about_on_its_own() {
        let local = answered(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:dataset:d1", "observedAt": "2020-08-01T11:00:00Z"}]
        }))
        .await;
        let probe = answered(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": 2, "datasetId": "urn:ngsi-ld:dataset:d2", "observedAt": "2020-08-01T11:40:00Z"}]
        }))
        .await;
        let datasets = follow_up_datasets(&[vec![&probe]], [&local]);
        pretty_assertions::assert_eq!(
            datasets,
            vec![DatasetSelector::Id("urn:ngsi-ld:dataset:d1".parse().expect("a valid identifier"))]
        );
    }

    /// A source that answered nothing for an attribute holds nothing of it
    /// before the window, in any dataset, so nothing is asked of it.
    #[tokio::test]
    async fn a_source_answering_nothing_is_asked_nothing_more() {
        let local = answered(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": 1, "observedAt": "2020-08-01T11:00:00Z"}]
        }))
        .await;
        pretty_assertions::assert_eq!(follow_up_datasets(&[Vec::new()], [&local]), Vec::<DatasetSelector>::new());
    }

    /// Each source is asked again for the datasets it alone left out: the
    /// first answered `d1`, the second `d2`, and the local part holds both.
    #[tokio::test]
    async fn each_source_is_asked_about_the_datasets_it_left_out() {
        let speed = |dataset: &str| {
            json!({
                "id": "urn:ngsi-ld:Vehicle:1",
                "type": "Vehicle",
                "speed": [{"type": "Property", "value": 1, "datasetId": dataset, "observedAt": "2020-08-01T11:00:00Z"}]
            })
        };
        let first = answered(&speed("urn:ngsi-ld:dataset:d1")).await;
        let second = answered(&speed("urn:ngsi-ld:dataset:d2")).await;
        let local = answered(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:dataset:d1", "observedAt": "2020-08-01T10:00:00Z"},
                {"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:dataset:d2", "observedAt": "2020-08-01T10:00:00Z"},
                {"type": "Property", "value": 1, "observedAt": "2020-08-01T10:00:00Z"}
            ]
        }))
        .await;
        let mut datasets: Vec<String> = follow_up_datasets(&[vec![&first], vec![&second]], [&local])
            .iter()
            .map(ToString::to_string)
            .collect();
        datasets.sort();
        pretty_assertions::assert_eq!(datasets, ["@none", "urn:ngsi-ld:dataset:d1", "urn:ngsi-ld:dataset:d2"]);
    }
}
