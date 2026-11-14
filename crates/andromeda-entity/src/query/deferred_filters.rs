//! The filters `splitEntities` holds back from the forwarded query (clause
//! 5.7.2.4).
//!
//! With `splitEntities`, an entity's attributes may be spread across several
//! context sources, so no single source can decide whether the entity matches.
//! The attribute-reading filters (`q`, the geoquery and `scopeQ`) therefore
//! stay with the broker: each source answers whatever it holds, and the filters
//! are applied here, once the local and forwarded entities have been merged.

use crate::{error::EntityError, query::EntityQueryService};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_linked_retrieval::{admission::LinkedOperation, filter_hops::FilterHops};
use andromeda_ngsi_ld_entity::{document::EntityDocument, queried::linked_states::LinkedStates};
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_query::{
    entity_filters::EntityFilters,
    filter::{
        compilation::{FilterAdmission, FilterOptIns, compile_q_filter},
        expression::Expr,
        language_targeting::LanguageTargeting,
    },
    geo::compile::compile_geo_query,
    term_expander::TermExpander,
};
use andromeda_ngsi_ld_query_evaluation::entity_filters::entity_filters_admit;
use andromeda_ngsi_ld_request::deferred_split_filters::DeferredSplitFilters;
use chrono::Utc;

impl EntityQueryService {
    /// Keep the merged `entities` the `deferred` filters admit.
    ///
    /// Clause 5.7.2.4: with `splitEntities` no single source held enough of an
    /// entity to decide `q`, the geoquery or `scopeQ`, so those are evaluated
    /// once here, over the merged result. The merged entities are already in
    /// the model, so the filters, expanded under `context`, are evaluated over
    /// the document each one is. The entities a `rel{…}` hop of `q` crosses
    /// into (clause 4.9) are read from `target` as they stand now, and its
    /// target languages are admitted and select keys under
    /// `language_targeting`, the level the request runs under.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError::Query`] when `q` or the geoquery cannot be
    /// compiled against `context`, [`EntityError::Request`] when a hop of `q`
    /// reaches past the request's `join`, and the store's [`EntityError`] when the
    /// entities a hop of `q` links to cannot be read.
    pub async fn retain_matching(
        &self,
        target: &ContextTarget,
        entities: Vec<ReceivedState>,
        mut deferred: DeferredSplitFilters,
        context: &ContextSpec,
        language_targeting: LanguageTargeting,
    ) -> Result<Vec<ReceivedState>, EntityError> {
        // One preparation serves the deferred `q` and the geoquery alike.
        let prepared_context = self.engine().prepare_term_context(context);
        let expander = ContextualExpander::new(self.engine(), &prepared_context);
        let q = match deferred.q.take() {
            Some(q_expr) => Some(self.compile_deferred_q(q_expr, &deferred, language_targeting, &expander).await?),
            None => None,
        };
        let geo = compile_geo_query(deferred.geo.as_ref(), &expander, self.geo_admission).await?;
        let filters = EntityFilters {
            q: q.as_ref(),
            language_matching: language_targeting.matching(),
            geo: geo.as_ref(),
            scope: deferred.scope_q.as_ref(),
        };

        let documents: Vec<(ReceivedState, EntityDocument)> = entities
            .into_iter()
            .map(|version| {
                let document = EntityDocument::from(&version.state);
                (version, document)
            })
            .collect();
        let linked = self.linked_states_of(target, &documents, filters.q).await?;
        Ok(documents
            .into_iter()
            .filter(|(_, document)| entity_filters_admit(filters, document, &linked))
            .map(|(version, _)| version)
            .collect())
    }

    /// Compile the deferred `q`, expanded under the opt-ins and the hop bound
    /// `deferred` carries.
    async fn compile_deferred_q(
        &self,
        q_expr: Expr,
        deferred: &DeferredSplitFilters,
        language_targeting: LanguageTargeting,
        expander: &impl TermExpander,
    ) -> Result<Expr, EntityError> {
        // Clause 4.9 EXAMPLE 11: the deferred filter is expanded under
        // the same `jsonKeys` declaration the local query used, so one
        // request has one expansion of one filter.
        let opt_ins = FilterOptIns::expanded(deferred.json_keys.as_deref(), deferred.expand_values.as_deref(), expander).await?;
        let admission = FilterAdmission {
            complexity: self.filter_complexity,
            language_targeting,
        };
        // Clause 5.7.2.4: `splitEntities` belongs to Query Entities.
        let hops = FilterHops::new(LinkedOperation::QueryEntities, deferred.join, deferred.join_level);
        compile_q_filter::<EntityError, _, _>(q_expr, &opt_ins, admission, &hops, expander).await
    }

    /// The entities the `rel{…}` hops of `q` cross into from the merged
    /// `documents`, none when `q` takes no hop.
    async fn linked_states_of(
        &self,
        target: &ContextTarget,
        documents: &[(ReceivedState, EntityDocument)],
        q: Option<&Expr>,
    ) -> Result<LinkedStates, EntityError> {
        match q.filter(|expr| expr.contains_linked_hop()) {
            Some(expr) => {
                let roots: Vec<&EntityDocument> = documents.iter().map(|(_, document)| document).collect();
                self.linked_filter_states(target, &roots, &[expr], Utc::now()).await
            }
            None => Ok(LinkedStates::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::query::service_test_double::service;
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
    use andromeda_ngsi_ld_model::entity_system_times::EntitySystemTimes;
    use andromeda_ngsi_ld_query::{
        filter::{language_targeting::LanguageTargeting, parser::parse},
        scope::query::ScopeQuery,
    };
    use andromeda_ngsi_ld_request::deferred_split_filters::DeferredSplitFilters;
    use serde_json::json;

    /// A merged Vehicle `id` whose `speed` is `speed`, in the scope `/Madrid`.
    fn vehicle(id: &str, speed: u32) -> ReceivedState {
        let document = json!([{
            "@id": id,
            "@type": ["https://example.com/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/Madrid"}],
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": speed}]
            }]
        }]);
        ReceivedState {
            state: EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored state"),
            system_times: EntitySystemTimes::default(),
            linked: Vec::new(),
        }
    }

    /// The merged entities `deferred` keeps of a fast and a slow Vehicle, by
    /// identifier.
    async fn kept_of_two_vehicles(deferred: DeferredSplitFilters) -> Vec<String> {
        service()
            .retain_matching(
                &ContextTarget::Live,
                vec![vehicle("urn:ngsi-ld:Vehicle:fast", 80), vehicle("urn:ngsi-ld:Vehicle:slow", 40)],
                deferred,
                &ContextSpec::core_only(),
                LanguageTargeting::Specification,
            )
            .await
            .expect("the deferred filters compile")
            .into_iter()
            .map(|version| version.state.id().as_str().to_owned())
            .collect()
    }

    #[tokio::test]
    async fn the_deferred_filters_keep_the_merged_entities_they_admit() {
        let deferred = DeferredSplitFilters {
            q: Some(parse("\"https://example.com/speed\">50").expect("q parses")),
            scope_q: Some(ScopeQuery::parse("/Madrid").expect("scopeQ parses")),
            ..DeferredSplitFilters::default()
        };
        pretty_assertions::assert_eq!(kept_of_two_vehicles(deferred).await, ["urn:ngsi-ld:Vehicle:fast"]);
    }

    #[tokio::test]
    async fn a_scope_query_no_merged_entity_meets_keeps_none() {
        let deferred = DeferredSplitFilters {
            scope_q: Some(ScopeQuery::parse("/Paris").expect("scopeQ parses")),
            ..DeferredSplitFilters::default()
        };
        pretty_assertions::assert_eq!(kept_of_two_vehicles(deferred).await, Vec::<String>::new());
    }

    #[tokio::test]
    async fn deferring_no_filter_keeps_every_merged_entity() {
        pretty_assertions::assert_eq!(
            kept_of_two_vehicles(DeferredSplitFilters::default()).await,
            ["urn:ngsi-ld:Vehicle:fast", "urn:ngsi-ld:Vehicle:slow"]
        );
    }
}
