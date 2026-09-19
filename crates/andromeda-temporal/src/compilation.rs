//! Temporal query compilation policy.

use crate::{error::TemporalError, service::TemporalService};
use andromeda_entity::query::selector_compilation::compile_entity_selectors;
use andromeda_jsonld::{context_spec::ContextSpec, engine::term_lookup::PreparedTermContext, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, temporal_format::TemporalFormat};
use andromeda_ngsi_ld_query::{
    entity_selection::{ConsideredEntities, EntitySelection},
    filter::{
        expansion::{apply_expand_values, expand_attr_path, expand_attribute_names, expand_q_expr},
        expression::Expr,
        json_key_attributes::JsonKeyAttributes,
    },
    geo::{compile as geo_compile, query::GeoQuery},
    pagination::{CountPolicy, Pagination},
    projection::{
        compiled::Projection,
        expansion::{expand_attrs, expand_projection_terms},
    },
    temporal::{
        compiled::CompiledTemporalQuery,
        grouping::{AggregationGrouping, GroupingTerm, group_key::GroupKey},
        instance_selection::InstanceSelection,
        last_n::LastNOrigin,
        ordering::TemporalEntityOrdering,
        representation::Representation,
        window::{TemporalWindow, TimeInterval},
    },
    term_expander::TermExpander,
};
use andromeda_ngsi_ld_request::{
    error::ParameterError,
    pagination_cursor::decode_cursor,
    query::{SharedQueryParams, temporal::TemporalQueryParameters},
};
use std::collections::HashSet;

impl TemporalService {
    /// Validate, expand, and compile temporal query parameters against one JSON-LD context.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Query`] when a selector, `q` filter, geo
    /// selection or projection term is not well formed, when the
    /// `timerel` / `timeAt` / `endTimeAt` triple does not describe an interval
    /// (clause 5.7.4.4), or when the ordering names something clause 5.7.4.3
    /// does not admit, and [`TemporalError::JsonLd`] when a term cannot be
    /// expanded against `context`.
    pub async fn compile_temporal_query(&self, params: &TemporalQueryParameters, context: &ContextSpec) -> Result<CompiledTemporalQuery, TemporalError> {
        let prepared_context = self.engine.prepare_term_context(context);
        let expander = ContextualExpander::new(&self.engine, &prepared_context);
        // Clause 5.2.33: a POST temporal query body's `entities[]` is a union of
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
        let projection = compile_projection(params, &expander).await?;

        let dataset_id_filter = params
            .dataset_id
            .as_deref()
            // The compiled query owns its selectors independently of the reusable request parameters.
            .map(ToOwned::to_owned)
            .unwrap_or_default();

        let q_expr = compile_q_filter(params, &expander).await?;
        let geo = self.compile_geo_query(params, &prepared_context).await?;

        let format = params.parse_format()?;
        let last_n = compile_last_n(params, format, self.default_last_n);
        let grouping = compile_grouping(params, format, &expander).await?;

        // Clause 5.5.9.1: the opaque pagination cursor is a per-request position
        // decoded here; it never took part in expansion, so it is set on the
        // compiled query rather than driving it. A malformed one is a
        // BadRequestData, lifted from the parameter error.
        let keyset = decode_cursor(params.cursor.as_deref())?;

        Ok(CompiledTemporalQuery::builder()
            .entities(
                EntitySelection::builder()
                    .selectors(selectors)
                    .q_expr(q_expr)
                    .geo(geo)
                    // clone: the compiled selection owns its scope query independently of the reusable request parameters.
                    .scope_q(params.scope_q.clone())
                    // Clause 5.5.14: a request narrowed to an empty identifier
                    // set considers no entity, which the selectors' identifier
                    // lists cannot say on their own.
                    .considered(ConsideredEntities::of_request(params.id.as_deref()))
                    .build(),
            )
            .window(
                TemporalWindow::builder()
                    .interval(TimeInterval::from_parts(params.timerel, params.time_at, params.end_time_at)?)
                    .timeproperty(params.timeproperty_or_default())
                    .build(),
            )
            .instances(
                InstanceSelection::builder()
                    .attr_filter(attr_filter)
                    .dataset_id_filter(dataset_id_filter)
                    .last_n(last_n.effective)
                    .last_n_origin(last_n.origin)
                    .build(),
            )
            .representation(compile_representation(params, format, grouping))
            .pagination(compile_pagination(params))
            .projection(projection)
            // Clause 5.7.4.5: "If Entity ordering is specified (see clause
            // 4.23), then the JSON-LD array returned shall be ordered according
            // to the member names specified." The ordering needs no context
            // expansion: clause 5.7.4.3 confines it to the `id` member, which
            // names no term.
            .ordering(TemporalEntityOrdering::of_parameters(&params.ordering)?)
            .keyset(keyset)
            .build())
    }

    /// Compile the optional temporal geo selection through the prepared request context.
    async fn compile_geo_query(&self, params: &TemporalQueryParameters, context: &PreparedTermContext) -> Result<Option<GeoQuery>, TemporalError> {
        geo_compile::compile_geo_query(params.geo.0.as_ref(), &ContextualExpander::new(&self.engine, context))
            .await
            .map_err(TemporalError::from)
    }
}

/// Expand the `q` filter and apply the attribute-scoped opt-ins to it.
async fn compile_q_filter(params: &TemporalQueryParameters, expander: &impl TermExpander) -> Result<Option<Expr>, TemporalError> {
    // Expansion consumes the AST while the caller retains its request parameters.
    let Some(expr) = params.q.clone() else {
        return Ok(None);
    };
    // `jsonKeys` (clause 4.9): the values of the listed attributes are raw
    // JSON, so the object keys the filter addresses under them stay literal
    // rather than being expanded against the supplied `@context`. That decides
    // how the filter's terms expand, so it is resolved first.
    let json_keys: JsonKeyAttributes = match params.json_keys.as_deref() {
        Some(attributes) => expand_attribute_names(attributes, expander).await?.into_iter().collect(),
        None => JsonKeyAttributes::default(),
    };
    let expr = expand_q_expr(expr, &json_keys, expander).await?;

    // `expandValues`: expand string right-hand-side values for the attributes
    // that opted in (spec temporal params).
    let expr = match params.expand_values.as_deref() {
        Some(attributes) => {
            let opted_in: HashSet<AttributeName> = expand_attribute_names(attributes, expander).await?.into_iter().collect();
            apply_expand_values(expr, &opted_in, expander).await?
        }
        None => expr,
    };
    Ok(Some(expr))
}

/// The instance cap a temporal read runs under, and where that cap came from.
struct LastNSelection {
    /// Number of instances kept per attribute, absent when none is imposed.
    effective: Option<u32>,
    /// Whether the cap is the client's own, a narrowed form of it, or the broker's.
    origin: LastNOrigin,
}

/// Resolve `lastN` against the broker's safety cap.
///
/// An aggregated representation is already bounded by its aggregation period
/// (clause 4.5.19), so the safety cap that clause 6.3.10 leaves to the broker
/// is imposed only on the instance-returning formats.
fn compile_last_n(params: &TemporalQueryParameters, format: TemporalFormat, cap: u32) -> LastNSelection {
    if format.is_aggregated() {
        return LastNSelection {
            effective: params.last_n,
            origin: match params.last_n {
                Some(requested) => LastNOrigin::UserSpecified { requested },
                Option::None => LastNOrigin::None,
            },
        };
    }
    LastNSelection {
        effective: Some(params.last_n.map_or(cap, |requested| requested.min(cap))),
        origin: match params.last_n {
            Some(requested) if requested > cap => LastNOrigin::UserSpecifiedCapped { requested },
            Some(requested) => LastNOrigin::UserSpecified { requested },
            Option::None => LastNOrigin::SystemCapped,
        },
    }
}

/// Compile the clause 4.5.19 response shape: output format, aggregation
/// methods and period, the extension grouping, and whether system attributes
/// survive.
fn compile_representation(params: &TemporalQueryParameters, format: TemporalFormat, grouping: Option<AggregationGrouping>) -> Representation {
    Representation::builder()
        .format(format)
        // clone: the compiled representation owns its methods independently of the reusable request parameters.
        .aggr_methods(params.aggr_methods.clone().unwrap_or_default())
        .aggr_period_duration(params.aggr_period_duration)
        .grouping(grouping)
        .sys_attrs(params.sys_attrs())
        .build()
}

/// Parse and expand the `aggrGroupBy` grouping (the extension, study §4.2).
///
/// Value-path keys are expanded against the request context so they match the
/// stored attribute IRIs, exactly as `attrs` and `q` are; member and bucket
/// keys carry no term to expand.
///
/// # Errors
///
/// Returns [`TemporalError::Request`] carrying
/// [`ParameterError::AggrGroupByInvalid`] when `aggrGroupBy` is not a well-formed
/// grouping (the request seam validated it already, so this is the re-parse the
/// compiled query is built from), and [`TemporalError::Query`] when a value-path
/// key cannot be expanded.
async fn compile_grouping(
    params: &TemporalQueryParameters,
    format: TemporalFormat,
    expander: &impl TermExpander,
) -> Result<Option<AggregationGrouping>, TemporalError> {
    let (true, Some(raw)) = (format.is_grouped(), params.aggr_group_by.as_deref()) else {
        return Ok(None);
    };
    let grouping = AggregationGrouping::parse(raw).map_err(|error| TemporalError::Request(ParameterError::AggrGroupByInvalid(error)))?;
    let json_keys = JsonKeyAttributes::default();
    let mut expanded_terms = Vec::with_capacity(grouping.terms().len());
    for term in grouping.0 {
        let key = match term.key {
            GroupKey::Member(member) => GroupKey::Member(member),
            GroupKey::Path(path) => GroupKey::Path(expand_attr_path(path, &json_keys, expander).await?),
        };
        expanded_terms.push(GroupingTerm { key, bucket: term.bucket });
    }
    Ok(Some(AggregationGrouping(expanded_terms)))
}

/// Compile the clause 5.5.9 page the query returns.
fn compile_pagination(params: &TemporalQueryParameters) -> Pagination {
    Pagination {
        limit: params.effective_limit(),
        offset: params.effective_offset(),
        count: match params.count {
            Some(true) => CountPolicy::Emit,
            Some(false) | None => CountPolicy::Omit,
        },
    }
}

/// Expand `pick` / `omit` into a compiled clause 4.21 [`Projection`].
///
/// Kept apart from the rest of [`TemporalService::compile_temporal_query`] so
/// the clause 4.5.23 join traversal (`crate::linked`) can expand the *root*
/// request's projection on its own, against its own freshly prepared
/// expander, without compiling the unrelated selection, window and
/// representation the traversal never reads.
pub(crate) async fn compile_projection(params: &TemporalQueryParameters, expander: &impl TermExpander) -> Result<Projection, TemporalError> {
    let pick = expand_projection_terms(params.pick.as_deref().unwrap_or_default(), expander).await?;
    let omit = expand_projection_terms(params.omit.as_deref().unwrap_or_default(), expander).await?;
    Ok(Projection { pick, omit })
}
