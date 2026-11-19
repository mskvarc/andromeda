//! The states one page of a snapshot entity capture leaves in the Snapshot's
//! branch (clauses 5.16.1.4 and 5.7.2.4).
//!
//! Every entity of a page is taken whole before the query decides on it: a
//! local state merged with what the Context Sources answered for it, or an
//! entity only a source holds, with no instance whose `expiresAt` has passed
//! (clause 4.22). Under `splitEntities` no reader holds enough of an entity to
//! decide `q`, the geoquery, `scopeQ` or the Attribute list, so they are
//! decided here, on the whole entity, as Query Entities decides them on its
//! merged result (clauses 4.3.6 and 5.7.2.4). Each entity the query selects is
//! then reduced to its `attrs`, `pick` or `omit` (clause 5.7.2.5), whatever
//! holder it came from. A `q` term reads every instance of an attribute,
//! whatever its `datasetId`, and an instance's `createdAt` and `modifiedAt`
//! (clause 4.9), so an entity is narrowed to the instances the query selects,
//! and one a source contributed to is stamped as a write of the capture
//! instant, only once the query has selected it.

use crate::{
    error::SnapshotError,
    executor::{
        SnapshotExecutor,
        captured_state::narrow_captured,
        execution::entity_query_failure,
        execution_failure::ExecutionFailure,
        forwarded_capture::ForwardedCapture,
        page_outcome::PageOutcome,
        status::failure_detail,
    },
};
use andromeda_entity::query::answer_list::AnswerList;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_query::dataset_selector::DatasetSelector;
use andromeda_ngsi_ld_representation::attribute_nesting::AttributeNestingExceeded;
use andromeda_ngsi_ld_request::deferred_split_filters::DeferredSplitFilters;
use chrono::{DateTime, Utc};
use derive_more::{AsMut, AsRef};

/// Where the state of a page's entity comes from, which decides how the
/// branch records it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StateOrigin {
    /// The local store alone holds it: the branch records the state as
    /// stored.
    Stored,
    /// A Context Source contributed to it: the branch records it as written
    /// at the capture instant.
    Received,
}

/// One entity of a captured page, before the query has selected it.
#[derive(AsMut, AsRef, Debug)]
pub(crate) struct CandidateEntity {
    /// The whole entity the query is decided on.
    #[as_mut]
    #[as_ref]
    pub(crate) state: EntityState,
    /// Where the state comes from.
    pub(crate) origin: StateOrigin,
}

impl CandidateEntity {
    /// A local state no Context Source contributed to.
    pub(crate) const fn stored(state: EntityState) -> CandidateEntity {
        CandidateEntity {
            state,
            origin: StateOrigin::Stored,
        }
    }

    /// An entity a Context Source contributed to, merged across every holder.
    pub(crate) const fn received(state: EntityState) -> CandidateEntity {
        CandidateEntity {
            state,
            origin: StateOrigin::Received,
        }
    }
}

/// What selects and narrows the entities of one capture.
pub(super) struct CaptureSelection<'q> {
    /// The filters `splitEntities` holds back to the whole entities, `None`
    /// when every reader applies the query's filters itself.
    pub(super) deferred: Option<DeferredSplitFilters>,
    /// The `attrs`, `pick` or `omit` every captured entity is reduced to, as
    /// Query Entities answers it (clause 5.7.2.5).
    pub(super) list: AnswerList<'q>,
    /// The context the query is expanded under.
    pub(super) context: &'q ContextSpec,
    /// The datasets the query selects instances of, `None` when it names
    /// none.
    pub(super) dataset_ids: Option<&'q [DatasetSelector]>,
}

impl SnapshotExecutor {
    /// The states the `candidates` of one page leave in the branch at
    /// `captured_at`, the ones the query selects narrowed to what its result
    /// holds, or the result the query ends with when the page cannot be
    /// captured whole.
    ///
    /// # Errors
    ///
    /// Returns a transient store failure the deferred filters met reading the
    /// entities a `q` hop links to.
    pub(super) async fn captured_page(
        &self,
        mut candidates: Vec<CandidateEntity>,
        selection: &CaptureSelection<'_>,
        capture: &ForwardedCapture,
        captured_at: DateTime<Utc>,
    ) -> Result<PageOutcome, SnapshotError> {
        remove_expired(&mut candidates, captured_at);
        let selected = match &selection.deferred {
            Some(deferred) => {
                let query = &self.sources.entity_query;
                match query
                    .answer_gathered(
                        &self.sources.live_target,
                        candidates,
                        deferred,
                        selection.list,
                        selection.context,
                        query.language_targeting(),
                    )
                    .await
                {
                    Ok(selected) => selected,
                    Err(error) => return entity_query_failure(error).map(PageOutcome::Ended),
                }
            }
            None => selection.list.answered(candidates, captured_at),
        };
        Ok(match branch_states(selected, selection.dataset_ids, capture, captured_at) {
            Ok(states) => PageOutcome::States(states),
            Err(error) => PageOutcome::Ended(failure_detail(ExecutionFailure::UncapturableSourceEntity { cause: &error }.problem())),
        })
    }
}

/// Remove from every candidate the instances whose `expiresAt` passed at
/// `captured_at` (clause 4.22).
fn remove_expired(candidates: &mut [CandidateEntity], captured_at: DateTime<Utc>) {
    for candidate in candidates {
        candidate.state.remove_expired_instances(captured_at);
    }
}

/// The states the branch holds for the `selected` candidates: each narrowed
/// to the instances `dataset_ids` selects, and one a source contributed to
/// written at `captured_at`.
///
/// # Errors
///
/// Returns [`AttributeNestingExceeded`] when a candidate a source contributed
/// to nests an attribute deeper than the branch could read back.
fn branch_states(
    selected: Vec<CandidateEntity>,
    dataset_ids: Option<&[DatasetSelector]>,
    capture: &ForwardedCapture,
    captured_at: DateTime<Utc>,
) -> Result<Vec<EntityState>, AttributeNestingExceeded> {
    selected
        .into_iter()
        .map(|candidate| {
            let mut state = candidate.state;
            narrow_captured(&mut state, dataset_ids);
            match candidate.origin {
                StateOrigin::Stored => Ok(state),
                StateOrigin::Received => capture.written(state, captured_at),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::executor::{
        captured_page::{CandidateEntity, StateOrigin, branch_states, remove_expired},
        forwarded_capture::ForwardedCapture,
    };
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::time::{date_time::NgsiLdDateTime, system_instant::SystemInstant};
    use andromeda_ngsi_ld_query::dataset_selector::DatasetSelector;
    use andromeda_ngsi_ld_representation::attribute_nesting::AttributeNestingLimit;
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};
    use std::num::NonZeroUsize;

    const SPEED: &str = "https://example.org/speed";
    const BRAND: &str = "https://example.org/brand";

    fn property(value: i64, dataset_id: Option<&str>, expires_at: Option<&str>) -> Value {
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

    fn candidate(speed: &[Value], brand: &[Value], origin: StateOrigin) -> CandidateEntity {
        let node = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.org/Vehicle"],
            SPEED: speed,
            BRAND: brand
        }]);
        CandidateEntity {
            state: EntityState::from_slice(node.to_string().as_bytes()).expect("a stored state"),
            origin,
        }
    }

    fn instant(text: &str) -> DateTime<Utc> {
        text.parse::<NgsiLdDateTime>().expect("an instant").get()
    }

    fn capture() -> ForwardedCapture {
        ForwardedCapture::new(AttributeNestingLimit::new(NonZeroUsize::new(4).expect("a positive limit")))
    }

    fn selection() -> Vec<DatasetSelector> {
        vec![DatasetSelector::Id("urn:ds:a".parse().expect("a dataset identifier"))]
    }

    /// The clause 4.8 times of every instance of `attribute`.
    fn stamps(state: &EntityState, attribute: &str) -> Vec<(Option<SystemInstant>, Option<SystemInstant>)> {
        state
            .attributes()
            .get(attribute)
            .map_or_default(|instances| instances.iter().map(|(_, instance)| (instance.created_at(), instance.modified_at())).collect())
    }

    /// Clause 4.22: an instance whose `expiresAt` passed before the capture is
    /// no part of the entity the query decides on, and an attribute left with
    /// none is gone.
    #[test]
    fn an_instance_expired_at_capture_time_is_removed_before_the_query_decides() {
        let mut candidates = [candidate(
            &[property(1, None, Some("2025-01-01T00:00:00Z"))],
            &[property(2, None, Some("2027-01-01T00:00:00Z"))],
            StateOrigin::Stored,
        )];
        remove_expired(&mut candidates, instant("2026-01-01T00:00:00Z"));
        assert!(!candidates[0].state.attributes().contains(SPEED));
        assert!(candidates[0].state.attributes().contains(BRAND));
    }

    /// A local state no source contributed to keeps the times it was stored
    /// with, narrowed to the selected datasets.
    #[test]
    fn a_stored_candidate_is_narrowed_and_keeps_its_stored_times() {
        let mut stored = candidate(
            &[property(1, Some("urn:ds:a"), None), property(2, Some("urn:ds:b"), None)],
            &[property(3, Some("urn:ds:b"), None)],
            StateOrigin::Stored,
        );
        let stored_at: SystemInstant = "2020-01-01T00:00:00Z".parse().expect("RFC 3339");
        stored.state.stamp_attribute_instants(None, stored_at);
        let selection = selection();
        let states = branch_states(vec![stored], Some(&selection), &capture(), Utc::now()).expect("captures");
        pretty_assertions::assert_eq!(stamps(&states[0], SPEED), vec![(Some(stored_at), Some(stored_at))]);
        assert!(!states[0].attributes().contains(BRAND), "brand held only an unselected instance");
    }

    /// An entity a source contributed to is narrowed to the selected datasets
    /// and written at the capture instant.
    #[test]
    fn a_received_candidate_is_narrowed_and_written_at_the_capture_instant() {
        let received = candidate(
            &[property(1, Some("urn:ds:a"), None), property(2, Some("urn:ds:b"), None)],
            &[property(3, Some("urn:ds:b"), None)],
            StateOrigin::Received,
        );
        let selection = selection();
        let captured_at = Utc::now();
        let states = branch_states(vec![received], Some(&selection), &capture(), captured_at).expect("captures");
        let at = Some(SystemInstant::new(captured_at));
        pretty_assertions::assert_eq!(stamps(&states[0], SPEED), vec![(at, at)]);
        assert!(!states[0].attributes().contains(BRAND), "brand held only an unselected instance");
    }
}
