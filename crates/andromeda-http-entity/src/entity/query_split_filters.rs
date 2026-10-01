//! The filters `splitEntities` holds back from the forwarded query (clause 5.7.2.4).
//!
//! With `splitEntities`, an entity's attributes may be spread across several
//! context sources, so no single source can decide whether the entity matches.
//! The attribute-reading filters — `q`, the geoquery and `scopeQ` — therefore
//! stay with the broker: each source answers whatever it holds, and the filters
//! are applied here, once the local and forwarded entities have been merged.

use crate::error::EntityResultExt;
use andromeda_entity::query::EntityQueryService;
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_model::{
    entity::Entity,
    expanded::query_match::{matches_geo_query, matches_scope_query},
};
use andromeda_ngsi_ld_query::{
    filter::{
        eval::evaluate,
        expansion::{expand_attribute_names, expand_q_expr},
        json_key_attributes::JsonKeyAttributes,
    },
    geo::compile::compile_geo_query,
};
use andromeda_ngsi_ld_request::deferred_split_filters::DeferredSplitFilters;
use axum::http::Uri as HttpUri;

/// Keep the merged entities the deferred filters match.
///
/// An entity that cannot be expanded into the request's context cannot be
/// matched against filters written in that context, so it is dropped rather
/// than admitted unfiltered.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when `q` or the geoquery cannot be compiled
/// against the request's context.
pub(crate) async fn retain_matching(
    service: &EntityQueryService,
    entities: Vec<Entity>,
    deferred: DeferredSplitFilters,
    context: &ContextSpec,
    instance: &HttpUri,
) -> Result<Vec<Entity>, ProblemResponse> {
    // One preparation serves the deferred `q` and the geoquery alike.
    let prepared_context = service.engine().prepare_term_context(context);
    let term_expander = ContextualExpander::new(service.engine(), &prepared_context);
    let compiled_q = match deferred.q {
        Some(q_expr) => {
            // Clause 4.9 EXAMPLE 11: the deferred filter is expanded under the
            // same `jsonKeys` declaration the local query used, so one request
            // has one expansion of one filter.
            let json_keys: JsonKeyAttributes = match deferred.json_keys {
                Some(ref declared) => expand_attribute_names(declared, &term_expander)
                    .await
                    .with_entity_instance(instance)?
                    .into_iter()
                    .collect(),
                None => JsonKeyAttributes::default(),
            };
            Some(expand_q_expr(q_expr, &json_keys, &term_expander).await.with_entity_instance(instance)?)
        }
        None => None,
    };
    let compiled_geo = compile_geo_query(deferred.geo.as_ref(), &term_expander, service.reference_limits())
        .await
        .with_entity_instance(instance)?;
    let compiled_scope = deferred.scope_q;

    let mut matching = Vec::with_capacity(entities.len());
    for entity in entities {
        let Ok(compact) = serde_json::to_value(&entity) else {
            continue;
        };
        let Ok(expanded) = service.engine().expand(compact, context).await else {
            continue;
        };

        let q_ok = compiled_q.as_ref().is_none_or(|expr| evaluate(expr, &expanded));
        let geo_ok = compiled_geo.as_ref().is_none_or(|geo| matches_geo_query(geo, &expanded));
        let scope_ok = compiled_scope.as_ref().is_none_or(|scope| matches_scope_query(scope, &expanded));

        if q_ok && geo_ok && scope_ok {
            matching.push(entity);
        }
    }
    Ok(matching)
}
