use crate::{
    temporal::query::{
        hub_aggregation::{HubAggregates, asked_documents, overlapping_entities, parts_by_entity, with_local_parts},
        merged_history::MergedTemporalHistory,
    },
    test_history::held_history,
};
use andromeda_federation::outcome::ForwardedTemporalEntities;
use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    registration::forwarding_mode::ForwardingMode,
    temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory},
};
use andromeda_temporal::merged_parts::LocalPart;
use serde_json::json;

fn id(raw: &str) -> Identifier {
    raw.parse().expect("a valid identifier")
}

/// The history of the vehicle `number`, holding `speed` readings `values`.
async fn vehicle(number: u32, values: &[u32]) -> TemporalHistory {
    let instances: Vec<_> = values
        .iter()
        .enumerate()
        .map(|(minute, value)| json!({"type": "Property", "value": value, "observedAt": format!("2024-01-01T00:{minute:02}:00Z")}))
        .collect();
    held_history(json!({"id": format!("urn:ngsi-ld:Vehicle:{number}"), "type": "Vehicle", "speed": instances})).await
}

/// The instance document of a history read from a source.
fn document(history: TemporalHistory) -> TemporalDocument {
    match history {
        TemporalHistory::Instances(document) => document,
        TemporalHistory::Aggregates(entity) => panic!("an instance history, not the aggregates of {}", entity.id),
    }
}

/// One source's answer holding `entities`.
fn answered(entities: Vec<TemporalHistory>) -> ForwardedTemporalEntities {
    ForwardedTemporalEntities {
        registration_id: None,
        mode: ForwardingMode::Inclusive,
        entities,
        warnings: Vec::new(),
        linked_map: None,
        partial_pages: Vec::new(),
    }
}

fn ids_of(entities: &[TemporalHistory]) -> Vec<String> {
    entities.iter().filter_map(|entity| entity.id().map(ToString::to_string)).collect()
}

/// Clause 5.7.4.4 merges every part into one entity before aggregating it: an
/// entity held locally and by a source, or by two sources, is held in more
/// than one part, and one only the local read or only one source holds is not.
#[tokio::test]
async fn an_entity_two_parts_hold_overlaps_and_one_a_single_part_holds_does_not() {
    let local = vec![vehicle(1, &[1]).await, vehicle(2, &[2]).await];
    let sources = vec![
        answered(vec![vehicle(2, &[3]).await, vehicle(3, &[4]).await]),
        answered(vec![vehicle(3, &[5]).await, vehicle(4, &[6]).await]),
    ];
    pretty_assertions::assert_eq!(
        overlapping_entities(&local, &sources),
        vec![id("urn:ngsi-ld:Vehicle:2"), id("urn:ngsi-ld:Vehicle:3")]
    );
}

/// A part is a holder, not an answered element: one source naming an entity
/// twice still holds one part of it.
#[tokio::test]
async fn one_source_naming_an_entity_twice_holds_one_part_of_it() {
    let sources = vec![answered(vec![vehicle(5, &[1]).await, vehicle(5, &[2]).await])];
    pretty_assertions::assert_eq!(overlapping_entities(&[], &sources), Vec::<Identifier>::new());
}

/// Three sources holding the same entity make it one overlap, named once.
#[tokio::test]
async fn an_entity_three_sources_hold_is_named_once() {
    let sources = vec![
        answered(vec![vehicle(6, &[1]).await]),
        answered(vec![vehicle(6, &[2]).await]),
        answered(vec![vehicle(6, &[3]).await]),
    ];
    pretty_assertions::assert_eq!(overlapping_entities(&[], &sources), vec![id("urn:ngsi-ld:Vehicle:6")]);
}

/// Every source's document of an entity joins that entity's part list, in the
/// order the sources answered, whatever order the entities came in.
#[tokio::test]
async fn the_documents_of_each_entity_are_gathered_under_it() {
    let documents = vec![
        document(vehicle(2, &[1, 2]).await),
        document(vehicle(1, &[3]).await),
        document(vehicle(2, &[4, 5, 6]).await),
    ];
    let parts = parts_by_entity(documents.into_iter());
    let gathered: Vec<(String, Vec<usize>)> = parts
        .iter()
        .map(|(entity_id, documents)| (entity_id.to_string(), documents.iter().map(TemporalDocument::instance_count).collect()))
        .collect();
    pretty_assertions::assert_eq!(
        gathered,
        vec![
            (String::from("urn:ngsi-ld:Vehicle:1"), vec![1]),
            (String::from("urn:ngsi-ld:Vehicle:2"), vec![2, 3]),
        ]
    );
}

/// An entity aggregated here takes the place of whatever the merge made of its
/// holders' aggregates; every other entity of the merge stays.
#[tokio::test]
async fn an_entity_aggregated_here_replaces_its_merged_holders() {
    let mut merged = MergedTemporalHistory {
        entities: vec![vehicle(1, &[1]).await, vehicle(2, &[2]).await, vehicle(3, &[3]).await],
        warnings: Vec::new(),
        contributions: Vec::new(),
    };
    let aggregated = HubAggregates {
        ids: [id("urn:ngsi-ld:Vehicle:2")].into_iter().collect(),
        entities: vec![vehicle(2, &[2, 4, 6]).await],
        warnings: vec![NgsiLdWarning::new(WarnCode::RevalidationFailed, String::from("unreadable part"))],
    };
    aggregated.replace_in(&mut merged);
    pretty_assertions::assert_eq!(
        ids_of(&merged.entities),
        vec!["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:3", "urn:ngsi-ld:Vehicle:2"]
    );
    pretty_assertions::assert_eq!(merged.entities.last().map(TemporalHistory::instance_count), Some(3));
    pretty_assertions::assert_eq!(merged.warnings.len(), 1);
}

/// Clause 5.7.4.4 selects S2 to S4 from the local entities before the merge
/// when entities are not split: an entity the local read left out failed the
/// filters locally, so only its answered parts are aggregated, and one the read
/// returned joins its local instances to them.
#[tokio::test]
async fn only_an_entity_the_local_read_returned_joins_its_local_part() {
    let local = vec![vehicle(1, &[60]).await];
    let parts = vec![
        (id("urn:ngsi-ld:Vehicle:1"), vec![document(vehicle(1, &[70, 80]).await)]),
        (id("urn:ngsi-ld:Vehicle:2"), vec![document(vehicle(2, &[90]).await)]),
    ];
    let joined: Vec<(String, LocalPart, Vec<usize>)> = with_local_parts(&local, parts)
        .iter()
        .map(|part| {
            (
                part.entity_id.to_string(),
                part.local,
                part.answered.iter().map(TemporalDocument::instance_count).collect(),
            )
        })
        .collect();
    pretty_assertions::assert_eq!(
        joined,
        vec![
            (String::from("urn:ngsi-ld:Vehicle:1"), LocalPart::Included, vec![2]),
            (String::from("urn:ngsi-ld:Vehicle:2"), LocalPart::Excluded, vec![1]),
        ]
    );
}

/// A source asked again for the overlapping entities may answer others besides; only the entities asked for are
/// aggregated here, so none the client's query did not select joins the response.
#[tokio::test]
async fn only_the_entities_asked_again_are_taken_from_the_second_round() {
    let sources = vec![answered(vec![vehicle(1, &[1, 2]).await, vehicle(2, &[3]).await])];
    let taken: Vec<String> = asked_documents(sources, &[id("urn:ngsi-ld:Vehicle:2")])
        .filter_map(|document| document.id().map(ToString::to_string))
        .collect();
    pretty_assertions::assert_eq!(taken, vec![String::from("urn:ngsi-ld:Vehicle:2")]);
}
