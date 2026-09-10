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
use andromeda_ngsi_ld_request::{deferred_split_filters::DeferredSplitFilters, query::entity::parameters::EntityQueryParameters};
use axum::http::Uri as HttpUri;

/// Take the attribute-reading filters out of the query, leaving it selecting
/// on identity alone so every source answers with what it holds.
///
/// `jsonKeys` leaves with the filter it governs: clause 4.9 makes it a
/// statement about how `q`'s own terms expand, and with `q` deferred it has
/// nothing left to say to the local query.
pub(crate) const fn defer_filters(query: &mut EntityQueryParameters) -> DeferredSplitFilters {
    DeferredSplitFilters {
        q: query.q.take(),
        geo: query.geo.0.take(),
        scope_q: query.scope_q.take(),
        json_keys: query.json_keys.take(),
    }
}

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
    let compiled_q = match deferred.q {
        Some(q_expr) => {
            let prepared_context = service.engine().prepare_term_context(context);
            let expander = ContextualExpander::new(service.engine(), &prepared_context);
            // Clause 4.9 EXAMPLE 11: the deferred filter is expanded under the
            // same `jsonKeys` declaration the local query used, so one request
            // has one expansion of one filter.
            let json_keys: JsonKeyAttributes = match deferred.json_keys {
                Some(ref declared) => expand_attribute_names(declared, &expander)
                    .await
                    .with_entity_instance(instance)?
                    .into_iter()
                    .collect(),
                None => JsonKeyAttributes::default(),
            };
            Some(expand_q_expr(q_expr, &json_keys, &expander).await.with_entity_instance(instance)?)
        }
        None => None,
    };
    let prepared_geo_context = service.engine().prepare_term_context(context);
    let geo_expander = ContextualExpander::new(service.engine(), &prepared_geo_context);
    let compiled_geo = compile_geo_query(deferred.geo.as_ref(), &geo_expander).await.with_entity_instance(instance)?;
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

#[cfg(test)]
mod tests {
    use crate::entity::query_split_filters::defer_filters;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::query::body::Query;
    use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
    use serde_json::json;

    /// The query a clause 5.2.23 body describes, which is the cheapest way to
    /// build a complete set of parameters.
    fn query_of(body: serde_json::Value) -> EntityQueryParameters {
        let body: Query = serde_json::from_value(body).expect("a valid clause 5.2.23 body");
        EntityQueryParameters::try_from(body).expect("an entity-applicable query")
    }

    /// Clause 4.9 EXAMPLE 11: with
    /// `?q=parkingTickets[value]=="Overstay 60 minutes"&jsonKeys=parkingTickets`
    /// "the target value raw is JSON, and is not expanded to ngsi-ld:hasValue
    /// using the core @context". The declaration decides how the deferred
    /// filter's own terms expand, so it has to travel with the filter: expanded
    /// without it, the residual filter addresses `ngsi-ld:hasValue` where the
    /// entity holds a literal `value` key and drops every entity that matches.
    #[test]
    fn the_json_keys_declaration_travels_with_the_filter_it_governs() {
        let mut query = query_of(json!({
            "type": "Query",
            "q": "parkingTickets[value]==\"Overstay 60 minutes\"",
            "jsonKeys": "parkingTickets"
        }));

        let deferred = defer_filters(&mut query);

        assert!(deferred.q.is_some(), "the filter itself is deferred");
        pretty_assertions::assert_eq!(
            deferred
                .json_keys
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(AttributeName::as_str)
                .collect::<Vec<_>>(),
            vec!["parkingTickets"]
        );
        assert!(
            query.json_keys.is_none(),
            "with `q` deferred the declaration has nothing left to say to the local query"
        );
    }

    /// A query that declared nothing defers nothing, and the expansion of its
    /// deferred filter is the one every filter without a declaration gets.
    #[test]
    fn a_query_without_a_declaration_defers_none() {
        let mut query = query_of(json!({"type": "Query", "q": "speed>50"}));

        let deferred = defer_filters(&mut query);

        assert!(deferred.q.is_some());
        assert!(deferred.json_keys.is_none());
    }
}
