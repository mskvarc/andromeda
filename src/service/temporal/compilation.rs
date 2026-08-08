//! temporal query compilation service behavior.

use crate::service::temporal::{
    AndromedaError,
    AttributeName,
    CompiledTemporalQuery,
    ContextSpec,
    ContextualExpander,
    CountPolicy,
    EntitySelection,
    GeoQuery,
    HashSet,
    InstanceSelection,
    IriBuf,
    LastNOrigin,
    Pagination,
    PreparedTermContext,
    QueryError,
    Representation,
    SharedQueryParams,
    SysAttrsInclusion,
    TemporalFormat,
    TemporalQueryParameters,
    TemporalService,
    TemporalWindow,
    TimeInterval,
    apply_expand_values,
    apply_json_keys,
    compile_entity_selectors,
    expand_attrs,
    expand_q_expr,
    expanded_attribute_name,
    geo_compile,
};

impl TemporalService {
    pub async fn compile_temporal_query(&self, params: &TemporalQueryParameters, context: &ContextSpec) -> Result<CompiledTemporalQuery, AndromedaError> {
        let prepared_context = self.engine.prepare_term_context(context);
        let expander = ContextualExpander::new(&self.engine, &prepared_context);
        // Clause 5.2.11: a POST temporal query body's `entities[]` is a union of
        // selectors, compiled by the same helper the entity path uses so the two
        // cannot disagree on what a selection means.
        let selectors = compile_entity_selectors(
            &self.engine,
            &prepared_context,
            &params.entity_selectors,
            params.id.clone().unwrap_or_default(),
            params.id_pattern.as_ref(),
            params.entity_type.as_ref(),
        )
        .await?;

        let attr_filter = expand_attrs(params.attrs.as_deref().unwrap_or_default(), &expander).await?;

        let dataset_id_filter = params
            .dataset_id
            .as_deref()
            // The compiled query owns its selectors independently of the reusable request parameters.
            .map(ToOwned::to_owned)
            .unwrap_or_default();

        let timerel = params.timerel;

        let aggr_period_duration = params.aggr_period_duration;

        // Expansion consumes the AST while the caller retains its request parameters.
        let q_expr = match params.q.clone() {
            Some(expr) => Some(expand_q_expr(expr, &expander).await?),
            None => None,
        };

        // Apply expandValues: expand string RHS values for specified attributes (spec temporal params)
        let q_expr = if let Some(q_expr) = q_expr {
            if let Some(ref ev) = params.expand_values {
                let ev_iris = expand_attrs(ev, &expander).await?;
                let ev_set = opted_in_attribute_names(&ev_iris)?;
                Some(apply_expand_values(q_expr, &ev_set, &expander).await?)
            } else {
                Some(q_expr)
            }
        } else {
            None
        };

        // Apply jsonKeys: expand sub-path keys for specified attributes (spec 6.4.3.2)
        let q_expr = if let Some(q_expr) = q_expr {
            if let Some(ref jk) = params.json_keys {
                let jk_iris = expand_attrs(jk, &expander).await?;
                let jk_set = opted_in_attribute_names(&jk_iris)?;
                Some(apply_json_keys(q_expr, &jk_set, &expander).await?)
            } else {
                Some(q_expr)
            }
        } else {
            None
        };

        let geo = self.compile_geo_query(params, &prepared_context).await?;

        let format = params.parse_format();
        let last_n_cap = self.default_last_n;
        let is_aggregated = format == TemporalFormat::AggregatedValues;
        let last_n_origin = if is_aggregated {
            if params.last_n.is_some() {
                LastNOrigin::UserSpecified
            } else {
                LastNOrigin::None
            }
        } else {
            match params.last_n {
                Some(n) if n > last_n_cap => LastNOrigin::UserSpecifiedCapped,
                Some(_) => LastNOrigin::UserSpecified,
                Option::None => LastNOrigin::SystemCapped,
            }
        };
        let effective_last_n = if !is_aggregated {
            Some(params.last_n.map_or(last_n_cap, |n| n.min(last_n_cap)))
        } else {
            params.last_n
        };

        Ok(CompiledTemporalQuery::builder()
            .entities(
                EntitySelection::builder()
                    .selectors(selectors)
                    .q_expr(q_expr)
                    .geo(geo)
                    .scope_q(params.scope_q.clone())
                    .build(),
            )
            .window(
                TemporalWindow::builder()
                    .interval(TimeInterval::from_parts(timerel, params.time_at, params.end_time_at)?)
                    .timeproperty(params.timeproperty_or_default())
                    .build(),
            )
            .instances(
                InstanceSelection::builder()
                    .attr_filter(attr_filter)
                    .dataset_id_filter(dataset_id_filter)
                    .last_n(effective_last_n)
                    .last_n_origin(last_n_origin)
                    .build(),
            )
            .representation(
                Representation::builder()
                    .format(format)
                    .aggr_methods(params.aggr_methods.clone().unwrap_or_default())
                    .aggr_period_duration(aggr_period_duration)
                    .sys_attrs(if params.has_sys_attrs() {
                        SysAttrsInclusion::Include
                    } else {
                        SysAttrsInclusion::Strip
                    })
                    .build(),
            )
            .pagination(Pagination {
                limit: params.effective_limit(),
                offset: params.effective_offset(),
                count: match params.count {
                    Some(true) => CountPolicy::Emit,
                    Some(false) | None => CountPolicy::Omit,
                },
            })
            .build())
    }

    async fn compile_geo_query(&self, params: &TemporalQueryParameters, context: &PreparedTermContext) -> Result<Option<GeoQuery>, AndromedaError> {
        geo_compile::compile_geo_query(params.geo.0.as_ref(), &ContextualExpander::new(&self.engine, context))
            .await
            .map_err(AndromedaError::from)
    }
}

fn opted_in_attribute_names(iris: &[IriBuf]) -> Result<HashSet<AttributeName>, QueryError> {
    iris.iter().map(expanded_attribute_name).collect()
}
