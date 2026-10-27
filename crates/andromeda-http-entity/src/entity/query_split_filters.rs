//! The filters `splitEntities` holds back from the forwarded query (clause 5.7.2.4).
//!
//! With `splitEntities`, an entity's attributes may be spread across several
//! context sources, so no single source can decide whether the entity matches.
//! The attribute-reading filters — `q`, the geoquery and `scopeQ` — therefore
//! stay with the broker: each source answers whatever it holds, and the filters
//! are applied here, once the local and forwarded entities have been merged.

use crate::{entity::query_forwarding::MergedEntities, error::EntityResultExt};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::EntityQueryService;
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_entity::{document::EntityDocument, queried::linked_states::LinkedStates};
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{
    filter::{
        eval::evaluate,
        expansion::{apply_expand_values, expand_attribute_names, expand_json_keys, expand_q_expr},
        language_matching::LanguageMatching,
    },
    geo::compile::compile_geo_query,
};
use andromeda_ngsi_ld_request::deferred_split_filters::DeferredSplitFilters;
use axum::http::Uri as HttpUri;
use chrono::Utc;
use std::collections::HashSet;

/// Keep the merged entities the deferred filters match, with the warnings and
/// contributions of the merge; a query that deferred nothing keeps every
/// entity the merge produced.
///
/// Clause 5.7.2.4: with `splitEntities` no single source held enough of an
/// entity to decide `q`, the geoquery or `scopeQ`, so those are evaluated once
/// here, over the merged result. The merged entities are already in the
/// model, so the filters, expanded under the request's context, are evaluated
/// over the document each one is. The entities a `rel{…}` hop of `q` crosses
/// into (clause 4.9) are read from `target` as they stand now, and its target
/// languages select keys by `language_matching`, the mode the request runs
/// under.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when `q` or the geoquery cannot be compiled
/// against the request's context, or when the entities a hop of `q` links to
/// cannot be read.
pub(crate) async fn retain_matching(
    service: &EntityQueryService,
    target: &ContextTarget,
    merged: MergedEntities,
    deferred: Option<DeferredSplitFilters>,
    context: &ContextSpec,
    language_matching: LanguageMatching,
    instance: &HttpUri,
) -> Result<MergedEntities, ProblemResponse> {
    let Some(deferred) = deferred else {
        return Ok(merged);
    };
    let MergedEntities {
        entities,
        warnings,
        contributions,
    } = merged;
    // One preparation serves the deferred `q` and the geoquery alike.
    let prepared_context = service.engine().prepare_term_context(context);
    let term_expander = ContextualExpander::new(service.engine(), &prepared_context);
    let compiled_q = match deferred.q {
        Some(q_expr) => {
            // Clause 4.9 EXAMPLE 11: the deferred filter is expanded under the
            // same `jsonKeys` declaration the local query used, so one request
            // has one expansion of one filter.
            let json_keys = expand_json_keys(deferred.json_keys.as_deref(), &term_expander)
                .await
                .with_entity_instance(instance)?;
            let opted_in: HashSet<AttributeName> = match deferred.expand_values {
                Some(ref declared) => expand_attribute_names(declared, &term_expander)
                    .await
                    .with_entity_instance(instance)?
                    .into_iter()
                    .collect(),
                None => HashSet::new(),
            };
            let expanded = expand_q_expr(q_expr, &json_keys, &term_expander).await.with_entity_instance(instance)?;
            Some(apply_expand_values(expanded, &opted_in, &term_expander).await.with_entity_instance(instance)?)
        }
        None => None,
    };
    let compiled_geo = compile_geo_query(deferred.geo.as_ref(), &term_expander, service.geo_admission())
        .await
        .with_entity_instance(instance)?;
    let compiled_scope = deferred.scope_q;

    let documents: Vec<(ReceivedState, EntityDocument)> = entities
        .into_iter()
        .map(|version| {
            let document = EntityDocument::from(&version.state);
            (version, document)
        })
        .collect();
    let linked = match compiled_q.as_ref().filter(|expr| expr.contains_linked_hop()) {
        Some(expr) => {
            let roots: Vec<&EntityDocument> = documents.iter().map(|(_, document)| document).collect();
            service
                .linked_filter_states(target, &roots, &[expr], Utc::now())
                .await
                .with_entity_instance(instance)?
        }
        None => LinkedStates::new(),
    };
    let mut matching = Vec::with_capacity(documents.len());
    for (entity, document) in documents {
        let q_ok = compiled_q.as_ref().is_none_or(|expr| evaluate(expr, &document, &linked, language_matching));
        let geo_ok = compiled_geo.as_ref().is_none_or(|geo| geo.matches_entity(&document));
        let scope_ok = compiled_scope.as_ref().is_none_or(|scope| scope.matches_entity(&document));

        if q_ok && geo_ok && scope_ok {
            matching.push(entity);
        }
    }
    Ok(MergedEntities {
        entities: matching,
        warnings,
        contributions,
    })
}
