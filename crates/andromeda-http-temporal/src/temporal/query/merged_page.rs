//! The page a temporal query answers with, cut from the merged history.
//!
//! Under `splitEntities` no single context source held enough of an entity to
//! decide `q`, the geoquery or the scope query (clause 5.7.4.4), so those
//! filters leave the local query and are evaluated once over the aggregation;
//! clause 5.5.9 then cuts the requested window from that same aggregation.

use crate::temporal::{
    ContextSpec,
    ContextualExpander,
    DeferredSplitFilters,
    JsonKeyAttributes,
    ProblemResponse,
    ResultInstanceExt,
    TemporalQueryParameters,
    compile_geo_query,
    evaluate,
    expand_attribute_names,
    expand_q_expr,
    matches_geo_query,
    matches_scope_query,
    query::{forwarding::ForwardedHistory, page::QueriedPage},
};
use andromeda_federation::{config::SplitEntities, temporal_last_n::truncate_to_last_n};
use andromeda_ngsi_ld_query::page_window::PageWindow;
use andromeda_temporal::service::TemporalService;
use axum::http::Uri;
use serde_json::Value;

/// Hold back the filters no single context source can decide on its own.
///
/// Clause 5.7.4.4: under `splitEntities` an entity's attributes may be spread
/// over several sources, so `q`, the geoquery and the scope query leave the
/// local query and are evaluated once over the merged result instead.
///
/// `jsonKeys` leaves with the filter it governs: clause 4.9 makes it a
/// statement about how `q`'s own terms expand, and with `q` deferred it has
/// nothing left to say to the local query.
pub(crate) fn defer_split_filters(params: &mut TemporalQueryParameters, filtering: SplitEntities) -> DeferredSplitFilters {
    match filtering {
        SplitEntities::Split => DeferredSplitFilters {
            q: params.q.take(),
            geo: params.geo.0.take(),
            scope_q: params.scope_q.take(),
            json_keys: params.json_keys.take(),
        },
        SplitEntities::Complete => DeferredSplitFilters::default(),
    }
}

/// Keep only the merged entities the deferred `splitEntities` filters accept.
///
/// Clause 5.7.4.4: `q`, the geoquery and the scope query are evaluated once
/// over the re-assembled entities, because no single source held enough of an
/// entity to decide them. An entity that cannot be expanded cannot be matched
/// against an expanded filter, so it does not reach the response.
///
/// # Errors
///
/// The classified failure of expanding the deferred `q` or geoquery.
async fn retain_split_matches(
    service: &TemporalService,
    context: &ContextSpec,
    deferred: DeferredSplitFilters,
    entities: Vec<Value>,
    instance: &Uri,
) -> Result<Vec<Value>, ProblemResponse> {
    // One preparation serves the deferred `q` and the geoquery alike.
    let engine = service.engine();
    let prepared_context = engine.prepare_term_context(context);
    let term_expander = ContextualExpander::new(engine, &prepared_context);
    let q_expr = match deferred.q {
        Some(q_expr) => {
            // Clause 4.9 EXAMPLE 11: the deferred filter is expanded under the
            // same `jsonKeys` declaration the local query used, so one request
            // has one expansion of one filter.
            let json_keys: JsonKeyAttributes = match deferred.json_keys {
                Some(ref declared) => expand_attribute_names(declared, &term_expander)
                    .await
                    .with_instance(instance)?
                    .into_iter()
                    .collect(),
                None => JsonKeyAttributes::default(),
            };
            Some(expand_q_expr(q_expr, &json_keys, &term_expander).await.with_instance(instance)?)
        }
        None => None,
    };
    let compiled_geo = compile_geo_query(deferred.geo.as_ref(), &term_expander, service.reference_limits())
        .await
        .with_instance(instance)?;
    let compiled_scope = deferred.scope_q;

    let mut retained = Vec::with_capacity(entities.len());
    for entity in entities {
        // clone: `expand` consumes the value, and the compact original is what the response carries
        let Ok(expanded) = engine.expand(entity.clone(), context).await else {
            continue;
        };

        let q_ok = q_expr.as_ref().is_none_or(|expr| evaluate(expr, &expanded));
        let geo_ok = compiled_geo.as_ref().is_none_or(|geo| matches_geo_query(geo, &expanded));
        let scope_ok = compiled_scope.as_ref().is_none_or(|scope| matches_scope_query(scope, &expanded));

        if q_ok && geo_ok && scope_ok {
            retained.push(entity);
        }
    }
    Ok(retained)
}

/// Bound the merged history and cut the requested page from it.
///
/// Clause 5.7.4.4: `lastN` bounds the instances of the aggregated history, so
/// it is applied once more over the merged entities; each source only bounded
/// its own part. Clause 5.5.9 then takes the page from the merged collection
/// rather than from the local rows, and clause 6.3.13 counts the merged
/// matches rather than the page.
///
/// The merge holds the local rows and then what each source answered, in
/// arrival order, and paging needs one deterministic order: the default `id`
/// order the local store pages in, byte-wise as the store collates
/// identifiers. No other order reaches this point, because clause 5.7.4.4
/// refuses an ordering on a query not limited to the local scope. The window
/// is the one the client asked for, by offset or by cursor alike.
fn merged_page(entities: Vec<Value>, params: &TemporalQueryParameters, window: &PageWindow) -> (Vec<Value>, Option<usize>) {
    let mut bounded = entities;
    if let Some(last_n) = params.last_n {
        let timeproperty = params.timeproperty_or_default();
        for entity in &mut bounded {
            truncate_to_last_n(entity, last_n, timeproperty);
        }
    }
    bounded.sort_by(|left, right| identifier_key(left).cmp(identifier_key(right)));
    let bounded = bounded;
    let total = params.count.unwrap_or(false).then_some(bounded.len());
    (window.cut(bounded, identifier_key), total)
}

/// The identifier a merged temporal entity is ordered by; one without a
/// readable `id` has the empty key and sorts first.
fn identifier_key(entity: &Value) -> &str {
    entity.get("id").and_then(Value::as_str).unwrap_or_default()
}

/// Hold the requested page back until after the merge when context sources
/// contributed entities, returning the window the merged collection is cut to.
///
/// Clause 5.5.9 cuts the page from the merged collection rather than from the
/// local rows, so the local query reads from the first row, past no cursor, and
/// as many rows as it takes to tell that the merge exceeds the request's budget
/// (clause 4.12).
pub(crate) fn hold_page_until_merge<'page>(
    params: &mut TemporalQueryParameters,
    forwarded: Option<&ForwardedHistory>,
    page: &'page QueriedPage,
) -> Option<&'page PageWindow> {
    let history = forwarded.filter(|history| history.holds_remote_entities())?;
    params.limit = Some(history.local_row_limit());
    params.offset = Some(0);
    params.cursor = None;
    Some(page.window())
}

/// What the local query held back until the context sources had answered.
///
/// Under `splitEntities` no single source held enough of an entity to decide
/// `q`, the geoquery or the scope query (clause 5.7.4.4), and clause 5.5.9
/// cuts the page from the merged collection rather than from the local rows.
/// Both decisions are therefore taken here, over the aggregation.
pub(crate) struct MergedPageCut<'page> {
    /// The filters that left the local query and are evaluated over the merge.
    pub(crate) deferred: DeferredSplitFilters,
    /// Whether an entity's attributes may be spread over several sources,
    /// which decides whether the deferred filters apply at all.
    pub(crate) filtering: SplitEntities,
    /// The page window withheld while the merge was pending, absent when no
    /// source contributed and the local query already cut the page.
    pub(crate) window: Option<&'page PageWindow>,
    /// The match count the local read reported, which stands when no page was
    /// withheld.
    pub(crate) local_total: Option<usize>,
}

/// Apply the deferred `splitEntities` filters (clause 5.7.4.4) to the merged
/// collection and cut the page the client asked for.
///
/// # Errors
///
/// The classified failure when a deferred filter cannot be compiled or
/// evaluated against a merged entity.
pub(crate) async fn cut_merged_page(
    service: &TemporalService,
    context: &ContextSpec,
    merged_entities: Vec<Value>,
    cut: MergedPageCut<'_>,
    params: &TemporalQueryParameters,
    instance: &Uri,
) -> Result<(Vec<Value>, Option<usize>), ProblemResponse> {
    let MergedPageCut {
        deferred,
        filtering,
        window,
        local_total,
    } = cut;
    let entities = match filtering {
        SplitEntities::Split => retain_split_matches(service, context, deferred, merged_entities, instance).await?,
        SplitEntities::Complete => merged_entities,
    };
    match window {
        Some(window) => Ok(merged_page(entities, params, window)),
        None => Ok((entities, local_total)),
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal::query::merged_page::defer_split_filters;
    use andromeda_federation::config::SplitEntities;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;

    /// A temporal query carrying the clause 4.9 EXAMPLE 11 filter and the
    /// declaration that governs it.
    fn declared_query() -> TemporalQueryParameters {
        TemporalQueryParameters {
            q: Some("parkingTickets[value]==\"Overstay 60 minutes\"".parse().expect("a clause 4.9 filter")),
            json_keys: Some(vec!["parkingTickets".parse().expect("a valid attribute name")]),
            ..TemporalQueryParameters::default()
        }
    }

    /// Clause 4.9 EXAMPLE 11: with `jsonKeys=parkingTickets` "the target value
    /// raw is JSON, and is not expanded to ngsi-ld:hasValue using the core
    /// @context". The declaration decides how the deferred filter's own terms
    /// expand, so one request has one expansion only if it travels with the
    /// filter it governs.
    #[test]
    fn the_json_keys_declaration_travels_with_the_filter_it_governs() {
        let mut params = declared_query();

        let deferred = defer_split_filters(&mut params, SplitEntities::Split);

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
            params.json_keys.is_none(),
            "with `q` deferred the declaration has nothing left to say to the local query"
        );
    }

    /// Without `splitEntities` nothing is deferred at all, so the local query
    /// keeps both the filter and the declaration that governs it.
    #[test]
    fn a_complete_query_defers_neither_the_filter_nor_its_declaration() {
        let mut params = declared_query();

        let deferred = defer_split_filters(&mut params, SplitEntities::Complete);

        assert!(deferred.q.is_none());
        assert!(deferred.json_keys.is_none());
        assert!(params.q.is_some() && params.json_keys.is_some());
    }
}
