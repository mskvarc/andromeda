//! The deletions a split temporal query still has to ask its sources for
//! (clauses 4.5.7 and 4.11).
//!
//! A source asked for the latest deletion before the window opens, on
//! `deletedAt`, answers the one with the latest `deletedAt` per Attribute and
//! `datasetId`. A deletion carrying no instant on the window's axis is placed
//! at its `deletedAt` (clause 4.5.7), which can fall after the placement of a
//! deletion with a later `deletedAt` that carries an `observedAt`, so the
//! deletion the first answer leaves out can still be the latest placement of
//! its stream. Clause 4.11 makes `before` an exclusive bound on the chosen
//! Temporal Property, so asking again with `before` at the `deletedAt` of the
//! earliest deletion returned reaches the next older one. The asking stops for
//! a stream when an answer holds no deletion, or when the `deletedAt` returned
//! is earlier than the latest placement already known for the stream, which no
//! older deletion can pass. The asking is the same whoever is asked, the
//! sources of a split query's roots or the holders of an entity a hop reaches,
//! so it runs over any answer that names its source.

use crate::in_effect_entity::placement;
use andromeda_ngsi_ld_entity::document::instance_document::InstanceDocument;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, time::date_time::NgsiLdDateTime};
use andromeda_ngsi_ld_model::temporal::temporal_document::TemporalDocument;
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    temporal::{earlier_deletions::EarlierDeletions, window::TemporalWindow},
};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use chrono::{DateTime, Utc};
use std::{collections::HashMap, iter};

/// One stream of an attribute of an entity: the `datasetId` it is written
/// under is `None` for the default instance (clause 4.5.5).
type StreamOf<'a> = (&'a Identifier, &'a AttributeName, Option<&'a Identifier>);

/// The source an answer came from and the parts it holds.
#[derive(Debug)]
pub struct SourcedDocuments<'a> {
    /// The registration of the source that answered; `None` for an answer
    /// naming none, which is never asked again.
    pub source: Option<&'a Identifier>,
    /// The parts the answer holds.
    pub documents: Vec<&'a TemporalDocument>,
}

/// Ask each source that answered `deletions` again with `ask`, for the older
/// deletions of the stream it answered, until every stream is followed to the
/// instance in effect when `window` opens; `deletions` and the answers to
/// every repeated request.
///
/// `sourced` reads an answer, and `held` is every other part known: the
/// instances in effect and the parts inside the window. The first answers
/// were asked for with `before` at the window's start.
///
/// # Errors
///
/// The first failure of `ask`.
pub async fn follow_earlier_deletions<A, E>(
    window: TemporalWindow,
    held: &[&TemporalDocument],
    mut deletions: Vec<A>,
    sourced: impl for<'x> Fn(&'x A) -> SourcedDocuments<'x>,
    mut ask: impl AsyncFnMut(&EarlierDeletions) -> Result<Vec<A>, E>,
) -> Result<Vec<A>, E> {
    let Some(start) = window.in_effect_start() else {
        return Ok(deletions);
    };
    let mut bounds = vec![start; deletions.len()];
    let mut latest_round = 0;
    loop {
        let requests = {
            let answers: Vec<DeletionAnswer<'_>> = deletions
                .iter()
                .zip(&bounds)
                .skip(latest_round)
                .filter_map(|(answer, bound)| {
                    let SourcedDocuments { source, documents } = sourced(answer);
                    Some(DeletionAnswer {
                        source: source?,
                        bound: *bound,
                        documents,
                    })
                })
                .collect();
            let known = held.iter().copied().chain(deletions.iter().flat_map(|answer| sourced(answer).documents));
            earlier_deletions(&answers, known, window)
        };
        if requests.is_empty() {
            return Ok(deletions);
        }
        latest_round = deletions.len();
        for request in &requests {
            let answers = ask(request).await?;
            bounds.extend(iter::repeat_n(request.before, answers.len()));
            deletions.extend(answers);
        }
    }
}

/// What one source answered a deletion request with.
#[derive(Debug)]
struct DeletionAnswer<'a> {
    /// The registration of the source that answered.
    source: &'a Identifier,
    /// The exclusive upper bound on `deletedAt` the request carried; a
    /// deletion at or after it answers some other request.
    bound: DateTime<Utc>,
    /// The parts the source answered with.
    documents: Vec<&'a TemporalDocument>,
}

/// The deletion requests to send next, given the answers to the last ones
/// (`answers`) and every part known so far (`known`: this broker's, those the
/// window and the in-effect probes were answered with, and every deletion
/// answered). Empty when the window has no instance in effect before it.
///
/// A source is sent a request for a stream only when it answered a deletion
/// for it, and one request covers every entity whose stream of the same
/// attribute and `datasetId` reached the same instant.
fn earlier_deletions<'a>(
    answers: &[DeletionAnswer<'a>],
    known: impl IntoIterator<Item = &'a TemporalDocument>,
    window: TemporalWindow,
) -> Vec<EarlierDeletions> {
    let Some(start) = window.in_effect_start() else {
        return Vec::new();
    };
    let mut latest: HashMap<StreamOf<'a>, DateTime<Utc>> = HashMap::new();
    for (stream, instance) in known.into_iter().flat_map(stream_instances) {
        if let Some(placed) = placement(instance, window.timeproperty).filter(|placed| *placed < start) {
            latest.entry(stream).and_modify(|latest| *latest = (*latest).max(placed)).or_insert(placed);
        }
    }
    let mut asked: Vec<EarlierDeletions> = Vec::new();
    for answer in answers {
        for ((entity, attribute, dataset), before) in earliest_deletions(answer) {
            if latest.get(&(entity, attribute, dataset)).is_some_and(|placed| before < *placed) {
                continue;
            }
            let dataset = dataset.map_or(DatasetSelector::Default, |dataset| DatasetSelector::Id(dataset.clone())); // clone: the request owns the dataset it names
            match asked
                .iter_mut()
                .find(|request| request.source == *answer.source && request.attribute == *attribute && request.dataset == dataset && request.before == before)
            {
                Some(request) => {
                    if !request.entities.contains(entity) {
                        request.entities.push(entity.clone()); // clone: the request owns the entities it names
                    }
                }
                None => asked.push(EarlierDeletions {
                    source: answer.source.clone(),  // clone: the request owns the registration it is sent to
                    entities: vec![entity.clone()], // clone: the request owns the entities it names
                    attribute: attribute.clone(),   // clone: the request owns the attribute it names
                    dataset,
                    before,
                }),
            }
        }
    }
    asked
}

/// The earliest `deletedAt` `answer` holds for each stream, among the
/// deletions below the bound its request carried.
fn earliest_deletions<'a>(answer: &DeletionAnswer<'a>) -> HashMap<StreamOf<'a>, DateTime<Utc>> {
    let mut earliest: HashMap<StreamOf<'a>, DateTime<Utc>> = HashMap::new();
    for (stream, instance) in answer.documents.iter().copied().flat_map(stream_instances) {
        let Some(deleted_at) = instance.deleted_at().map(NgsiLdDateTime::get).filter(|deleted_at| *deleted_at < answer.bound) else {
            continue;
        };
        earliest
            .entry(stream)
            .and_modify(|earliest| *earliest = (*earliest).min(deleted_at))
            .or_insert(deleted_at);
    }
    earliest
}

/// Every Attribute instance of `part` with the stream it belongs to; the Scope
/// is no Attribute and has no dataset (clause 4.5.4).
fn stream_instances(part: &TemporalDocument) -> impl Iterator<Item = (StreamOf<'_>, &InstanceDocument)> {
    let entity = part.id();
    part.instances()
        .filter(|(attribute, _)| attribute.as_str() != expanded_properties::SCOPE.as_str())
        .filter_map(move |(attribute, instance)| Some(((entity?, attribute, instance.dataset_id()), instance)))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        answered_fixture::answered,
        deletion_follow_up::{DeletionAnswer, earlier_deletions},
    };
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::date_time::NgsiLdDateTime, time_prop::TimeProp};
    use andromeda_ngsi_ld_model::temporal::temporal_document::TemporalDocument;
    use andromeda_ngsi_ld_query::{
        dataset::DatasetSelector,
        temporal::window::{TemporalWindow, TimeInterval},
    };
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};

    fn at(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
    }

    /// The window from 12:00 to 13:00 on `observedAt`.
    fn noon_to_one() -> TemporalWindow {
        TemporalWindow::builder()
            .timeproperty(TimeProp::Observed)
            .interval(TimeInterval::Between {
                from: at("2020-08-01T12:00:00Z"),
                to: at("2020-08-01T13:00:00Z"),
            })
            .build()
    }

    fn source(raw: &str) -> Identifier {
        raw.parse().expect("a valid identifier")
    }

    /// Vehicle 1 holding `speed` instances.
    async fn speed(instances: &Value) -> TemporalDocument {
        answered(&json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": instances})).await
    }

    /// The `speed` instance in effect at 12:00, observed at 11:00.
    fn in_effect() -> Value {
        json!({"type": "Property", "value": 60, "observedAt": "2020-08-01T11:00:00Z"})
    }

    /// A deletion of `speed` carrying no `observedAt`, placed at `deleted_at`.
    fn undated_deletion(deleted_at: &str) -> Value {
        json!({"type": "Property", "value": "urn:ngsi-ld:null", "deletedAt": deleted_at})
    }

    /// A deletion of `speed` observed at `observed_at`, deleted at `deleted_at`.
    fn dated_deletion(observed_at: &str, deleted_at: &str) -> Value {
        json!({"type": "Property", "value": "urn:ngsi-ld:null", "observedAt": observed_at, "deletedAt": deleted_at})
    }

    /// Clause 4.5.7 with clause 4.11: the first answer holds the dated
    /// deletion with the latest `deletedAt`, placed at 10:00, below the
    /// instance in effect, while an undated deletion with an older `deletedAt`
    /// is placed at 11:20 and ends the stream. The source is asked again before
    /// the `deletedAt` of the one it returned.
    #[tokio::test]
    async fn a_dated_deletion_with_a_later_deleted_at_is_followed_up() {
        let registration = source("urn:ngsi-ld:csr:1");
        let effect = speed(&json!([in_effect()])).await;
        let dated = speed(&json!([dated_deletion("2020-08-01T10:00:00Z", "2020-08-01T11:50:00Z")])).await;
        let answers = [DeletionAnswer {
            source: &registration,
            bound: at("2020-08-01T12:00:00Z"),
            documents: vec![&dated],
        }];
        let asked = earlier_deletions(&answers, [&effect, &dated], noon_to_one());
        pretty_assertions::assert_eq!(asked.len(), 1);
        pretty_assertions::assert_eq!(asked[0].source, registration);
        pretty_assertions::assert_eq!(asked[0].dataset, DatasetSelector::Default);
        pretty_assertions::assert_eq!(asked[0].before, at("2020-08-01T11:50:00Z"));
        pretty_assertions::assert_eq!(asked[0].entities, [source("urn:ngsi-ld:Vehicle:1")]);
    }

    /// The undated deletion the follow-up reached is placed at 11:20, after the
    /// instance in effect, so the asking goes on before it.
    #[tokio::test]
    async fn an_undated_deletion_placed_after_the_in_effect_instance_is_followed_up() {
        let registration = source("urn:ngsi-ld:csr:1");
        let effect = speed(&json!([in_effect()])).await;
        let undated = speed(&json!([undated_deletion("2020-08-01T11:20:00Z")])).await;
        let answers = [DeletionAnswer {
            source: &registration,
            bound: at("2020-08-01T11:50:00Z"),
            documents: vec![&undated],
        }];
        let asked = earlier_deletions(&answers, [&effect, &undated], noon_to_one());
        pretty_assertions::assert_eq!(asked.len(), 1);
        pretty_assertions::assert_eq!(asked[0].before, at("2020-08-01T11:20:00Z"));
    }

    /// An undated deletion placed before the instance in effect cannot end the
    /// stream, and no deletion older than it can either.
    #[tokio::test]
    async fn a_deletion_older_than_the_in_effect_instance_is_not_followed_up() {
        let registration = source("urn:ngsi-ld:csr:1");
        let effect = speed(&json!([in_effect()])).await;
        let undated = speed(&json!([undated_deletion("2020-08-01T10:00:00Z")])).await;
        let answers = [DeletionAnswer {
            source: &registration,
            bound: at("2020-08-01T12:00:00Z"),
            documents: vec![&undated],
        }];
        pretty_assertions::assert_eq!(earlier_deletions(&answers, [&effect, &undated], noon_to_one()), []);
    }

    /// A source that answered no deletion is asked nothing more.
    #[tokio::test]
    async fn a_source_answering_no_deletion_is_asked_nothing() {
        let registration = source("urn:ngsi-ld:csr:1");
        let effect = speed(&json!([in_effect()])).await;
        let answers = [DeletionAnswer {
            source: &registration,
            bound: at("2020-08-01T12:00:00Z"),
            documents: Vec::new(),
        }];
        pretty_assertions::assert_eq!(earlier_deletions(&answers, [&effect], noon_to_one()), []);
    }

    /// A deletion at or after the bound of the request answers some other
    /// request, and following it would never end.
    #[tokio::test]
    async fn a_deletion_at_or_after_the_bound_is_not_followed_up() {
        let registration = source("urn:ngsi-ld:csr:1");
        let effect = speed(&json!([in_effect()])).await;
        let undated = speed(&json!([undated_deletion("2020-08-01T11:20:00Z")])).await;
        let answers = [DeletionAnswer {
            source: &registration,
            bound: at("2020-08-01T11:20:00Z"),
            documents: vec![&undated],
        }];
        pretty_assertions::assert_eq!(earlier_deletions(&answers, [&effect, &undated], noon_to_one()), []);
    }

    /// Each source is followed up on its own: only the ones that answered a
    /// deletion still worth following are asked, each with the instant of its
    /// own answer. The stream is judged across the sources, so a deletion
    /// placed after the one a source answered ends that source's asking.
    #[tokio::test]
    async fn each_source_is_followed_up_on_its_own_answer() {
        let first = source("urn:ngsi-ld:csr:1");
        let second = source("urn:ngsi-ld:csr:2");
        let third = source("urn:ngsi-ld:csr:3");
        let effect = speed(&json!([in_effect()])).await;
        let from_first = speed(&json!([undated_deletion("2020-08-01T11:40:00Z")])).await;
        let from_second = speed(&json!([dated_deletion("2020-08-01T10:00:00Z", "2020-08-01T11:45:00Z")])).await;
        let bound = at("2020-08-01T12:00:00Z");
        let answers = [
            DeletionAnswer {
                source: &first,
                bound,
                documents: vec![&from_first],
            },
            DeletionAnswer {
                source: &second,
                bound,
                documents: vec![&from_second],
            },
            DeletionAnswer {
                source: &third,
                bound,
                documents: Vec::new(),
            },
        ];
        let mut asked = earlier_deletions(&answers, [&effect, &from_first, &from_second], noon_to_one());
        asked.sort_by(|left, right| left.source.as_str().cmp(right.source.as_str()));
        let requests: Vec<(&Identifier, DateTime<Utc>)> = asked.iter().map(|request| (&request.source, request.before)).collect();
        pretty_assertions::assert_eq!(requests, [(&first, at("2020-08-01T11:40:00Z")), (&second, at("2020-08-01T11:45:00Z"))]);
    }

    /// A deletion placed after the one a source answered ends the asking of
    /// that source, whichever source holds it.
    #[tokio::test]
    async fn a_deletion_placed_later_by_another_source_ends_the_asking() {
        let first = source("urn:ngsi-ld:csr:1");
        let second = source("urn:ngsi-ld:csr:2");
        let effect = speed(&json!([in_effect()])).await;
        let from_first = speed(&json!([undated_deletion("2020-08-01T11:30:00Z")])).await;
        let from_second = speed(&json!([undated_deletion("2020-08-01T11:40:00Z")])).await;
        let bound = at("2020-08-01T12:00:00Z");
        let answers = [
            DeletionAnswer {
                source: &first,
                bound,
                documents: vec![&from_first],
            },
            DeletionAnswer {
                source: &second,
                bound,
                documents: vec![&from_second],
            },
        ];
        let asked = earlier_deletions(&answers, [&effect, &from_first, &from_second], noon_to_one());
        pretty_assertions::assert_eq!(asked.len(), 1);
        pretty_assertions::assert_eq!(asked[0].source, second);
    }

    /// Clause 4.5.5: each `datasetId` is a stream of its own, with its own
    /// instance in effect, so the dataset whose deletion lies below its
    /// instance in effect is not followed up while the other is.
    #[tokio::test]
    async fn each_dataset_is_followed_up_against_its_own_in_effect_instance() {
        let registration = source("urn:ngsi-ld:csr:1");
        let dataset = |id: &str, mut instance: Value| {
            instance["datasetId"] = json!(id);
            instance
        };
        let effect = speed(&json!([
            dataset("urn:ngsi-ld:dataset:d1", in_effect()),
            dataset(
                "urn:ngsi-ld:dataset:d2",
                json!({"type": "Property", "value": 60, "observedAt": "2020-08-01T09:00:00Z"})
            )
        ]))
        .await;
        let deleted = speed(&json!([
            dataset("urn:ngsi-ld:dataset:d1", undated_deletion("2020-08-01T10:30:00Z")),
            dataset("urn:ngsi-ld:dataset:d2", undated_deletion("2020-08-01T10:30:00Z"))
        ]))
        .await;
        let answers = [DeletionAnswer {
            source: &registration,
            bound: at("2020-08-01T12:00:00Z"),
            documents: vec![&deleted],
        }];
        let asked = earlier_deletions(&answers, [&effect, &deleted], noon_to_one());
        pretty_assertions::assert_eq!(asked.len(), 1);
        pretty_assertions::assert_eq!(asked[0].dataset, DatasetSelector::Id(source("urn:ngsi-ld:dataset:d2")));
    }

    /// A window with no start has no instance in effect to protect.
    #[tokio::test]
    async fn a_window_without_a_start_asks_nothing() {
        let registration = source("urn:ngsi-ld:csr:1");
        let undated = speed(&json!([undated_deletion("2020-08-01T11:20:00Z")])).await;
        let before = TemporalWindow::builder()
            .timeproperty(TimeProp::Observed)
            .interval(TimeInterval::Before(at("2020-08-01T13:00:00Z")))
            .build();
        let answers = [DeletionAnswer {
            source: &registration,
            bound: at("2020-08-01T13:00:00Z"),
            documents: vec![&undated],
        }];
        pretty_assertions::assert_eq!(earlier_deletions(&answers, [&undated], before), []);
    }
}
