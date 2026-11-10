//! What the Context Sources answered one snapshot entity query, held until a
//! local page claims it or the capture's last pages take it, and the entity
//! limit the merged collection is held to (clause 5.16.1.4).
//!
//! A capture executes its query following clause 5.7.2.4, so the collection
//! it captures holds each entity once however many sources and local pages
//! answer it, and Query Entities refuses that collection with
//! `TooManyResults` once it exceeds the entity limit the sources were walked
//! under (clause 4.12). The local entities arrive page by page, so the merged
//! collection is counted as it grows: the local entities read so far, and
//! every source entity no local page has claimed yet.

use andromeda_federation::{
    outcome::ForwardedEntities,
    result_budget::{ResultBudget, ResultBudgetExceeded},
};
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use std::collections::HashSet;

/// What every matching Context Source answered, not yet captured.
pub(crate) struct PendingSources {
    /// The answers, each still holding the entities no local page has claimed.
    pub(crate) answers: Vec<ForwardedEntities>,
}

impl PendingSources {
    /// Take out of every answer the entities one local page holds, keeping
    /// each answer's registration and mode so the merge treats them as the
    /// answer they came from (clause 4.3.6.2).
    pub(crate) fn claim<'i>(&mut self, local: impl IntoIterator<Item = &'i Identifier>) -> Vec<ForwardedEntities> {
        let held: HashSet<&Identifier> = local.into_iter().collect();
        self.answers
            .iter_mut()
            .filter_map(|answer| {
                let (claimed, kept): (Vec<ReceivedState>, Vec<ReceivedState>) =
                    answer.entities.drain(..).partition(|received| held.contains(received.state.id()));
                answer.entities = kept;
                (!claimed.is_empty()).then(|| ForwardedEntities {
                    registration_id: answer.registration_id.clone(), // clone: the answer keeps its registration for the entities it still holds
                    mode: answer.mode,
                    entities: claimed,
                    warnings: Vec::new(),
                    linked_map: None,
                })
            })
            .collect()
    }

    /// Whether any answer still holds an entity.
    pub(crate) fn holds_entities(&self) -> bool {
        self.answers.iter().any(|answer| !answer.entities.is_empty())
    }

    /// Whether the merged collection fits the entity limit of `budget`: the
    /// `local` entities read from the store, every page already claimed from
    /// included, and the distinct entities the answers still hold. A source
    /// entity a local page held was claimed with it, so it counts once, as
    /// the local entity.
    ///
    /// # Errors
    ///
    /// Returns [`ResultBudgetExceeded::DistinctEntities`] when the merged
    /// collection exceeds the entity limit.
    pub(crate) fn admit_merged(&self, local: usize, budget: &ResultBudget) -> Result<(), ResultBudgetExceeded> {
        let unclaimed: HashSet<&Identifier> = self
            .answers
            .iter()
            .flat_map(|answer| answer.entities.iter().map(|received| received.state.id()))
            .collect();
        let merged = local.saturating_add(unclaimed.len());
        budget.admit_collection(u64::try_from(merged).unwrap_or(u64::MAX))
    }
}

#[cfg(test)]
mod tests {
    use crate::executor::pending_sources::PendingSources;
    use andromeda_federation::{
        outcome::ForwardedEntities,
        result_budget::{ResultBudget, ResultBudgetExceeded, ResultLimits},
    };
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{entity_system_times::EntitySystemTimes, registration::forwarding_mode::ForwardingMode};
    use serde_json::json;
    use std::num::NonZeroU32;

    fn entity(id: &str) -> ReceivedState {
        let stored = json!([{"@id": id, "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"]}]);
        ReceivedState {
            state: EntityState::from_slice(stored.to_string().as_bytes()).expect("a stored state"),
            system_times: EntitySystemTimes::default(),
            linked: Vec::new(),
        }
    }

    fn identifier(id: &str) -> Identifier {
        id.parse().expect("a valid identifier")
    }

    fn answer(ids: &[&str]) -> ForwardedEntities {
        ForwardedEntities {
            registration_id: Some("urn:ngsi-ld:ContextSourceRegistration:1".parse().expect("a valid URI")),
            mode: ForwardingMode::Inclusive,
            entities: ids.iter().map(|id| entity(id)).collect(),
            warnings: Vec::new(),
            linked_map: None,
        }
    }

    fn ids(answers: &[ForwardedEntities]) -> Vec<&str> {
        answers
            .iter()
            .flat_map(|answer| answer.entities.iter().map(|received| received.state.id().as_str()))
            .collect()
    }

    fn limit(entities: u32) -> NonZeroU32 {
        NonZeroU32::new(entities).expect("a positive limit")
    }

    /// A budget of `entities` distinct entities whose row limit no test
    /// reaches: local entities are never charged as rows.
    fn budget(entities: u32) -> ResultBudget {
        ResultBudget::new(ResultLimits {
            distinct_entities: limit(entities),
            source_rows: limit(u32::MAX),
        })
    }

    /// A local page claims exactly the source entities it holds; the others
    /// stay pending for a later page or for the capture after the last one.
    #[test]
    fn a_local_page_claims_only_the_source_entities_it_holds() {
        let mut pending = PendingSources {
            answers: vec![answer(&["urn:a", "urn:b", "urn:c"]), answer(&["urn:c", "urn:d"])],
        };
        let local = [identifier("urn:c"), identifier("urn:x")];
        let claimed = pending.claim(&local);
        pretty_assertions::assert_eq!(ids(&claimed), vec!["urn:c", "urn:c"]);
        pretty_assertions::assert_eq!(ids(&pending.answers), vec!["urn:a", "urn:b", "urn:d"]);
        assert!(pending.holds_entities());
    }

    /// Once every source entity is claimed nothing is pending.
    #[test]
    fn nothing_is_pending_once_every_source_entity_is_claimed() {
        let mut pending = PendingSources {
            answers: vec![answer(&["urn:a"])],
        };
        let local = [identifier("urn:a")];
        let claimed = pending.claim(&local);
        pretty_assertions::assert_eq!(ids(&claimed), vec!["urn:a"]);
        assert!(!pending.holds_entities());
    }

    /// Three local entities and three source entities each fit a limit of 5;
    /// merged they are 6 distinct entities, which Query Entities refuses
    /// (clauses 4.12 and 5.7.2.4).
    #[test]
    fn local_and_source_entities_fitting_the_limit_separately_but_not_together_are_refused() {
        let mut pending = PendingSources {
            answers: vec![answer(&["urn:d", "urn:e", "urn:f"])],
        };
        let local = [identifier("urn:a"), identifier("urn:b"), identifier("urn:c")];
        let claimed = pending.claim(&local);
        assert!(claimed.is_empty());
        pretty_assertions::assert_eq!(
            pending.admit_merged(local.len(), &budget(5)),
            Err(ResultBudgetExceeded::DistinctEntities { maximum: limit(5) })
        );
    }

    /// A local entity a source also answered is one entity of the merged
    /// collection (clause 5.7.2.4), so three local and three source entities
    /// sharing one are 5, within a limit of 5.
    #[test]
    fn a_local_entity_a_source_also_answered_counts_once() {
        let mut pending = PendingSources {
            answers: vec![answer(&["urn:c", "urn:d", "urn:e"])],
        };
        let local = [identifier("urn:a"), identifier("urn:b"), identifier("urn:c")];
        pending.claim(&local);
        pretty_assertions::assert_eq!(pending.admit_merged(local.len(), &budget(5)), Ok(()));
    }

    /// An entity two sources answer is one entity of the merged collection.
    #[test]
    fn an_entity_two_sources_answer_counts_once() {
        let pending = PendingSources {
            answers: vec![answer(&["urn:a", "urn:b"]), answer(&["urn:b", "urn:c"])],
        };
        pretty_assertions::assert_eq!(pending.admit_merged(0, &budget(3)), Ok(()));
        pretty_assertions::assert_eq!(
            pending.admit_merged(1, &budget(3)),
            Err(ResultBudgetExceeded::DistinctEntities { maximum: limit(3) })
        );
    }

    /// Later local pages keep counting the entities earlier pages claimed:
    /// the merged collection only grows as the capture reads on.
    #[test]
    fn the_entities_earlier_pages_claimed_stay_counted() {
        let mut pending = PendingSources {
            answers: vec![answer(&["urn:a", "urn:z"])],
        };
        let first_page = [identifier("urn:a"), identifier("urn:b")];
        pending.claim(&first_page);
        pretty_assertions::assert_eq!(pending.admit_merged(first_page.len(), &budget(3)), Ok(()));
        let second_page = [identifier("urn:c")];
        pending.claim(&second_page);
        pretty_assertions::assert_eq!(
            pending.admit_merged(first_page.len() + second_page.len(), &budget(3)),
            Err(ResultBudgetExceeded::DistinctEntities { maximum: limit(3) })
        );
    }
}
