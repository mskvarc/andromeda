//! Temporal query compilation policy.

use crate::{error::TemporalError, instance_limits::TemporalInstanceLimits, service::TemporalService};
use andromeda_entity::query::selector_compilation::compile_entity_selectors;
use andromeda_jsonld::{context_spec::ContextSpec, engine::term_lookup::PreparedTermContext, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, temporal_format::TemporalFormat};
use andromeda_ngsi_ld_query::{
    entity_selection::{ConsideredEntities, EntitySelection},
    filter::{
        expansion::{apply_expand_values, expand_attr_path, expand_attribute_names, expand_q_expr},
        expression::Expr,
        json_key_attributes::JsonKeyAttributes,
        trailing_members::TrailingMembers,
    },
    geo::{compile as geo_compile, query::GeoQuery},
    projection::{
        compiled::Projection,
        expansion::{expand_attrs, expand_projection_terms},
        projected_members::ProjectedMembers,
    },
    temporal::{
        aggregation_period::AggregationPeriod,
        compiled::CompiledTemporalQuery,
        grouping::{AggregationGrouping, GroupingTerm, group_key::GroupKey},
        instance_page::InstancePage,
        instance_selection::InstanceSelection,
        ordering::TemporalEntityOrdering,
        period_budget::PeriodBudget,
        representation::Representation,
    },
    term_expander::TermExpander,
};
use andromeda_ngsi_ld_request::{error::ParameterError, query::temporal::TemporalQueryParameters};
use std::{collections::HashSet, num::NonZeroU32};

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
        let page = compile_instance_page(params.last_n, format, params.aggr_period_duration, self.limits.instance_limits);
        let grouping = compile_grouping(params, format, &expander).await?;

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
                    // Clause 5.7.4.5: an entity the projection reduces to no
                    // member is not selected, so it is left out before the page.
                    .projected_members(ProjectedMembers::of(&projection))
                    .build(),
            )
            .window(params.window()?)
            .instances(
                InstanceSelection::builder()
                    .attr_filter(attr_filter)
                    .dataset_id_filter(dataset_id_filter)
                    .page(page)
                    .build(),
            )
            .representation(compile_representation(params, format, grouping))
            .projection(projection)
            // Clause 5.7.4.5: "If Entity ordering is specified (see clause
            // 4.23), then the JSON-LD array returned shall be ordered according
            // to the member names specified." The ordering needs no context
            // expansion: clause 5.7.4.3 confines it to the `id` member, which
            // names no term.
            .ordering(TemporalEntityOrdering::of_parameters(&params.ordering)?)
            .build())
    }

    /// Expand the request's clause 4.21 `pick` / `omit` projection against one
    /// JSON-LD context, which narrows the members a response writes of every
    /// entity (clause 5.7.3.5).
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Query`] when a projection term is not well
    /// formed, and [`TemporalError::JsonLd`] when a term cannot be expanded
    /// against `context`.
    pub async fn request_projection(&self, params: &TemporalQueryParameters, context: &ContextSpec) -> Result<Projection, TemporalError> {
        let prepared_context = self.engine.prepare_term_context(context);
        compile_projection(params, &ContextualExpander::new(&self.engine, &prepared_context)).await
    }

    /// Compile the optional temporal geo selection through the prepared request context.
    ///
    /// A reference geometry holding more positions than the broker resolves
    /// makes the query the `TooComplexQuery` of clause 5.5.6.
    async fn compile_geo_query(&self, params: &TemporalQueryParameters, context: &PreparedTermContext) -> Result<Option<GeoQuery>, TemporalError> {
        Ok(geo_compile::compile_geo_query(params.geo.0.as_ref(), &ContextualExpander::new(&self.engine, context), self.geo_admission()).await?)
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
    // that opted in, and refuse a bare word of any other.
    let opted_in: HashSet<AttributeName> = match params.expand_values.as_deref() {
        Some(attributes) => expand_attribute_names(attributes, expander).await?.into_iter().collect(),
        None => HashSet::new(),
    };
    Ok(Some(apply_expand_values(expr, &opted_in, expander).await?))
}

/// Resolve `lastN` against the broker's page sizes (clause 6.3.10).
///
/// A read without `lastN` is the forward default page; a `lastN` within the
/// maximum is answered in full; and one above it is narrowed to the maximum and
/// paged backwards, the `Content-Range` still reporting the requested number.
///
/// An aggregated representation pages by whole periods instead (clause
/// 4.5.19), at most the maximum of them and the instance budget per attribute,
/// forwards without `lastN` and backwards with one above the maximum. `lastN`
/// keeps its meaning of Table 5.2.21-1, the last n instances per attribute,
/// so the periods aggregate those alone, and a `lastN` within the maximum
/// yields at most that many periods and is answered in full. An aggregation
/// over the whole time range is one period (clause 4.5.19.1) and is never
/// paged.
const fn compile_instance_page(
    last_n: Option<NonZeroU32>,
    format: TemporalFormat,
    period: Option<AggregationPeriod>,
    limits: TemporalInstanceLimits,
) -> InstancePage {
    let budget = PeriodBudget {
        periods: limits.maximum,
        instances: limits.aggregation_instances,
    };
    match (format.is_aggregated(), period, last_n) {
        (true, None, None) => InstancePage::Whole,
        (true, Some(_), None) => InstancePage::Periods { budget },
        (false, _, None) => InstancePage::Forward { size: limits.default_page },
        (true, Some(_), Some(requested)) if requested.get() > limits.maximum => InstancePage::PeriodsBackward { budget, requested },
        (false, _, Some(requested)) if requested.get() > limits.maximum => InstancePage::Backward {
            size: limits.maximum,
            requested,
        },
        (_, _, Some(n)) => InstancePage::LastN { n },
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

/// Parse and expand the `aggrGroupBy` grouping (an Andromeda extension).
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
            GroupKey::Path(path) => GroupKey::Path(expand_attr_path(path, TrailingMembers::Terms(&json_keys), expander).await?),
        };
        expanded_terms.push(GroupingTerm { key, bucket: term.bucket });
    }
    Ok(Some(AggregationGrouping(expanded_terms)))
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

#[cfg(test)]
mod tests {
    use crate::{compilation::compile_instance_page, instance_limits::TemporalInstanceLimits};
    use andromeda_ngsi_ld_lexicon::temporal_format::TemporalFormat;
    use andromeda_ngsi_ld_query::temporal::{aggregation_period::AggregationPeriod, instance_page::InstancePage, period_budget::PeriodBudget};
    use std::num::{NonZeroU32, NonZeroU64};

    /// `n` as a client `lastN`.
    fn count(n: u32) -> NonZeroU32 {
        NonZeroU32::new(n).expect("a positive count")
    }

    const LIMITS: TemporalInstanceLimits = TemporalInstanceLimits {
        default_page: 100,
        maximum: 1000,
        edge_group: 100_000,
        aggregation_instances: NonZeroU64::MIN.saturating_add(49_999),
    };

    /// The budget of an aggregated page under [`LIMITS`]: the maximum in
    /// periods, and the instance budget.
    fn budget() -> PeriodBudget {
        PeriodBudget {
            periods: 1000,
            instances: NonZeroU64::new(50_000).expect("a positive budget"),
        }
    }

    /// An hour, the period these aggregated reads bucket by.
    fn hourly() -> Option<AggregationPeriod> {
        AggregationPeriod::parse("PT1H").expect("a valid period")
    }

    /// Clause 6.3.10: without `lastN` the read pages forwards, at the default
    /// page, rather than returning the most recent instances as an implicit
    /// `lastN`.
    #[test]
    fn a_read_without_last_n_is_a_forward_default_page() {
        pretty_assertions::assert_eq!(
            compile_instance_page(None, TemporalFormat::Normalized, None, LIMITS),
            InstancePage::Forward { size: 100 }
        );
        pretty_assertions::assert_eq!(
            compile_instance_page(None, TemporalFormat::TemporalValues, None, LIMITS),
            InstancePage::Forward { size: 100 }
        );
    }

    /// A `lastN` the maximum admits is the whole answer, the maximum included.
    #[test]
    fn a_last_n_within_the_maximum_is_answered_in_full() {
        for n in [1, 150, 1000].map(count) {
            pretty_assertions::assert_eq!(
                compile_instance_page(Some(n), TemporalFormat::Normalized, None, LIMITS),
                InstancePage::LastN { n }
            );
        }
    }

    /// A `lastN` above the maximum is narrowed and paged backwards, keeping the
    /// requested number for the `Content-Range` size.
    #[test]
    fn a_last_n_above_the_maximum_is_a_backward_page_of_the_maximum() {
        pretty_assertions::assert_eq!(
            compile_instance_page(Some(count(5000)), TemporalFormat::Normalized, None, LIMITS),
            InstancePage::Backward {
                size: 1000,
                requested: count(5000)
            }
        );
    }

    /// Clauses 4.5.19 and 6.3.10: a period is the element of the aggregated
    /// representation, so a read without `lastN` pages forwards by whole
    /// periods, at most the maximum of them and the instance budget per
    /// attribute, in the grouped format as in the standard one.
    #[test]
    fn an_aggregated_read_compiles_a_period_page() {
        for format in [TemporalFormat::AggregatedValues, TemporalFormat::GroupedValues] {
            pretty_assertions::assert_eq!(
                compile_instance_page(None, format, hourly(), LIMITS),
                InstancePage::Periods { budget: budget() },
                "{format:?}"
            );
        }
    }

    /// Table 5.2.21-1: `lastN` keeps the last n instances per attribute, which
    /// the periods then aggregate. Within the maximum they are at most that
    /// many periods and answered in full; above it the periods are paged
    /// backwards under the same budget, the `Content-Range` reporting the
    /// requested number.
    #[test]
    fn an_aggregated_read_with_last_n_narrows_its_instances() {
        for n in [1, 5, 1000].map(count) {
            pretty_assertions::assert_eq!(
                compile_instance_page(Some(n), TemporalFormat::AggregatedValues, hourly(), LIMITS),
                InstancePage::LastN { n }
            );
        }
        pretty_assertions::assert_eq!(
            compile_instance_page(Some(count(5000)), TemporalFormat::AggregatedValues, hourly(), LIMITS),
            InstancePage::PeriodsBackward {
                budget: budget(),
                requested: count(5000)
            }
        );
    }

    /// Clause 4.5.19.1: an aggregation over the whole time range is one
    /// period, so it is never paged, and a `lastN` of any size narrows the
    /// instances it aggregates.
    #[test]
    fn a_whole_range_aggregation_is_never_paged() {
        pretty_assertions::assert_eq!(compile_instance_page(None, TemporalFormat::AggregatedValues, None, LIMITS), InstancePage::Whole);
        for n in [5, 5000].map(count) {
            pretty_assertions::assert_eq!(
                compile_instance_page(Some(n), TemporalFormat::AggregatedValues, None, LIMITS),
                InstancePage::LastN { n }
            );
        }
    }
}
