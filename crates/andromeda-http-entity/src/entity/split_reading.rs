//! What `splitEntities` leaves an entity query's local read, its merge and its
//! Entity Map to decide (clause 5.7.2.4).
//!
//! Under split entities the local query matches the identifier, id pattern
//! and type alone, and `q`, the geoquery, `scopeQ` and the Attribute list are
//! applied to the aggregated Entities, which only then are reduced by the
//! request's list (clause 5.7.2.5). While no Context Source answered an
//! entity, the local entities are the aggregated ones, so the store decides
//! them where it cuts the page and counts the collection.

use crate::entity::{EntityResultExt, ProblemResponse, query_forwarding::ForwardedQueryResults};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::{EntityQueryService, answer_list::AnswerList, local_read::LocalRead};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_query::{candidate_filters::CandidateFilters, filter::language_targeting::LanguageTargeting};
use andromeda_ngsi_ld_request::{deferred_split_filters::DeferredSplitFilters, query::entity::admitted::AdmittedEntityQuery};
use axum::http::Uri as HttpUri;

/// Whether an entity query runs under `splitEntities`, and the filters it
/// holds back to the merged entities.
pub(crate) struct SplitReading {
    /// What the local candidates of the query are held to: deferred under
    /// `splitEntities`, which the Entity Map the query mints lists them by.
    candidates: CandidateFilters,
    /// The filters held back from the local read once a source answered an
    /// entity, `None` while the local read decides every filter.
    deferred: Option<DeferredSplitFilters>,
}

impl SplitReading {
    /// What `splitEntities`, in force when `split` says, leaves `query` to
    /// decide once the sources answered with `forwarded`; the held-back
    /// filters are taken out of `query`.
    pub(crate) fn of(query: &mut AdmittedEntityQuery, split: CandidateFilters, forwarded: Option<&ForwardedQueryResults>) -> SplitReading {
        let gathered = split == CandidateFilters::Deferred && forwarded.is_some_and(ForwardedQueryResults::holds_remote_entities);
        SplitReading {
            candidates: split,
            deferred: gathered.then(|| query.defer_split_filters()),
        }
    }

    /// Whether the query runs under `splitEntities`.
    pub(crate) fn is_split(&self) -> bool {
        self.candidates == CandidateFilters::Deferred
    }

    /// The local page read of `query`, its terms expanded under `context` and
    /// its target languages selecting keys under `language_targeting`.
    pub(crate) const fn page_read<'q>(&self, query: &'q AdmittedEntityQuery, context: &'q ContextSpec, language_targeting: LanguageTargeting) -> LocalRead<'q> {
        LocalRead {
            query,
            context,
            language_targeting,
            filters: match self.deferred {
                Some(_) => CandidateFilters::Deferred,
                None => CandidateFilters::Applied,
            },
        }
    }

    /// The read the Entity Map the query mints lists the local entities of
    /// `page_read` by: every local candidate under `splitEntities`, as each
    /// source contributes every entity it answered with (clauses 5.7.2.4 and
    /// 5.14.4.4).
    pub(crate) const fn map_listing<'q>(&self, page_read: LocalRead<'q>) -> LocalRead<'q> {
        LocalRead {
            filters: self.candidates,
            ..page_read
        }
    }

    /// The list the merge reduces the entities by: none while the held-back
    /// filters have still to decide on the whole merged entities.
    pub(crate) const fn merge_list<'q>(&self, list: AnswerList<'q>) -> AnswerList<'q> {
        match self.deferred {
            Some(_) => AnswerList::UNRESTRICTED,
            None => list,
        }
    }

    /// The merged `entities` the query answers: those the held-back filters
    /// admit, reduced to `list`, or `entities` as merged when nothing was held
    /// back. The filters are read as `local` reads the query.
    ///
    /// # Errors
    ///
    /// The classified failure of compiling the held-back filters, or of
    /// reading the entities a hop of `q` links to.
    pub(crate) async fn answered(
        self,
        service: &EntityQueryService,
        target: &ContextTarget,
        entities: Vec<ReceivedState>,
        list: AnswerList<'_>,
        local: LocalRead<'_>,
        instance: &HttpUri,
    ) -> Result<Vec<ReceivedState>, ProblemResponse> {
        match self.deferred {
            Some(deferred) => service
                .answer_gathered(target, entities, &deferred, list, local.context, local.language_targeting)
                .await
                .with_entity_instance(instance),
            None => Ok(entities),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::{query_forwarding::ForwardedQueryResults, split_reading::SplitReading};
    use andromeda_federation::{
        outcome::ForwardedEntities,
        result_budget::{ResultBudget, ResultLimits},
    };
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
    use andromeda_ngsi_ld_model::{entity_system_times::EntitySystemTimes, query::body::Query, registration::forwarding_mode::ForwardingMode};
    use andromeda_ngsi_ld_query::{
        candidate_filters::CandidateFilters,
        filter::{compilation::FilterAdmission, complexity::FilterComplexityLimits, language_targeting::LanguageTargeting},
    };
    use andromeda_ngsi_ld_request::query::entity::{admitted::AdmittedEntityQuery, operation::EntityQueryOperation, parameters::EntityQueryParameters};
    use serde_json::json;
    use std::num::NonZeroU32;

    /// A query filtering by `q`, admitted for Query Entities.
    fn filtered() -> AdmittedEntityQuery {
        let body: Query = serde_json::from_value(json!({"type": "Query", "q": "speed>50"})).expect("a valid clause 5.2.23 body");
        let admission = FilterAdmission {
            complexity: FilterComplexityLimits::PARSED,
            language_targeting: LanguageTargeting::Specification,
        };
        EntityQueryParameters::try_from(body)
            .expect("an entity-applicable query")
            .admit(EntityQueryOperation::Query, admission)
            .expect("a query validation admits")
    }

    /// What the sources answered with: `entities` from one inclusive source.
    fn forwarded(entities: Vec<ReceivedState>) -> ForwardedQueryResults {
        let limit = NonZeroU32::new(100).expect("a positive limit");
        ForwardedQueryResults {
            primary: vec![ForwardedEntities {
                registration_id: None,
                mode: ForwardingMode::Inclusive,
                entities,
                warnings: Vec::new(),
                linked_map: None,
            }],
            auxiliary: Vec::new(),
            warnings: Vec::new(),
            contributions: Vec::new(),
            budget: ResultBudget::new(ResultLimits {
                distinct_entities: limit,
                source_rows: limit,
            }),
        }
    }

    fn vehicle() -> ReceivedState {
        let document = json!([{"@id": "urn:ngsi-ld:Vehicle:1", "@type": ["https://example.com/Vehicle"]}]);
        ReceivedState {
            state: EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored state"),
            system_times: EntitySystemTimes::default(),
            linked: Vec::new(),
        }
    }

    /// Clause 5.7.2.4: once a source answered an entity, the local read takes
    /// each part whole and the filters wait for the merged entities.
    #[test]
    fn a_split_query_a_source_answered_defers_its_filters_to_the_merged_entities() {
        let mut query = filtered();
        let reading = SplitReading::of(&mut query, CandidateFilters::Deferred, Some(&forwarded(vec![vehicle()])));
        let context = ContextSpec::core_only();

        assert!(query.q.is_none(), "q waits for the merged entities");
        pretty_assertions::assert_eq!(
            reading.page_read(&query, &context, LanguageTargeting::Specification).filters,
            CandidateFilters::Deferred
        );
    }

    /// While no source answered an entity the local entities are the merged
    /// ones, so the local read decides every filter; the minted map still
    /// lists every local candidate.
    #[test]
    fn a_split_query_no_source_answered_is_decided_by_the_local_read() {
        let mut query = filtered();
        let reading = SplitReading::of(&mut query, CandidateFilters::Deferred, Some(&forwarded(Vec::new())));
        let context = ContextSpec::core_only();
        let page_read = reading.page_read(&query, &context, LanguageTargeting::Specification);

        assert!(query.q.is_some(), "the local read decides q");
        pretty_assertions::assert_eq!(page_read.filters, CandidateFilters::Applied);
        pretty_assertions::assert_eq!(reading.map_listing(page_read).filters, CandidateFilters::Deferred);
    }

    /// Without split entities every holder decides the filters, the local
    /// read and the map listing alike.
    #[test]
    fn an_unsplit_query_holds_every_read_to_its_filters() {
        let mut query = filtered();
        let reading = SplitReading::of(&mut query, CandidateFilters::Applied, Some(&forwarded(vec![vehicle()])));
        let context = ContextSpec::core_only();
        let page_read = reading.page_read(&query, &context, LanguageTargeting::Specification);

        assert!(query.q.is_some());
        assert!(!reading.is_split());
        pretty_assertions::assert_eq!(page_read.filters, CandidateFilters::Applied);
        pretty_assertions::assert_eq!(reading.map_listing(page_read).filters, CandidateFilters::Applied);
    }
}
