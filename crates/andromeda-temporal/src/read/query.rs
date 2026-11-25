//! Query Temporal Evolution of Entities (clause 5.7.4): the matching entities of
//! one page, each rendered at the page's common clause 6.3.10 cut.

use crate::{
    error::TemporalError,
    outcome::TemporalEntityIdOutcome,
    port::{
        history_reader::ShapeHolders,
        read_scope::{ScopeReach, TemporalReadScope},
    },
    query_reading::QueryReading,
    query_warning_reason::TemporalQueryWarningReason,
    read::{
        aggregated_page::{AggregationRendering, render_aggregation_page, settle_aggregation_pages},
        dataset_step::DatasetStep,
        rendered_entity::{HistoryPresence, RenderedTemporalEntity, render_history_page},
        unread_entity::UnreadEntity,
    },
    service::{TemporalQueryResult, TemporalQueryWarning, TemporalService},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_compaction::entity_compactor::EntityCompactor;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    page_window::PageWindow,
    projection::attrs_selection::AttrsAdmission,
    temporal::{compiled::CompiledTemporalQuery, contributed_entities::ContributedEntities, instance_page::InstancePage},
};
use andromeda_ngsi_ld_request::query::temporal::admitted::AdmittedTemporalRequest;
use andromeda_telemetry::temporal::{self, TemporalOperation};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

impl TemporalService {
    /// Query the page `window` selects of the temporal entities matching the
    /// given filter and temporal parameters, counting every match when `count`
    /// asks for it (clause 6.3.13).
    ///
    /// The request's terms and `q` target languages are read as `reading`
    /// says.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Request`] when the parameters name an attribute
    /// that is ineligible for a requested aggregation method (clause 5.7.4.4),
    /// and [`TemporalError::Persistence`]
    /// when the history cannot be read. A single entity that fails to render is reported as a warning
    /// on the result rather than failing the query.
    ///
    /// `contributed` names the entities a Context Source contributed a part of
    /// to a split query (clause 5.7.4.4): they are exempt from the value, geo
    /// and scope filters here, and their parts are judged together by
    /// [`TemporalService::settle_split_filters`]. One the response renders
    /// from its merged instances is selected but not rendered here.
    pub async fn query(
        &self,
        target: &ContextTarget,
        params: &AdmittedTemporalRequest,
        window: &PageWindow,
        count: CountPolicy,
        reading: QueryReading<'_>,
        contributed: ContributedEntities,
    ) -> Result<TemporalQueryResult, TemporalError> {
        let context = reading.context;
        let timer = temporal::start(TemporalOperation::Query);
        let result = async {
            let mut compiled = self.compile_temporal_query(params, reading, AttrsAdmission::ReifiedScopeAdmitted).await?;
            compiled.contributed = contributed;
            let compiled = compiled;

            let scope = self.reader.read_scope(target, ScopeReach::Every).await?;
            self.ensure_requested_attributes_aggregable(&scope, &compiled, ShapeHolders::Selected).await?;
            // Clause 5.5.9.1: the identifier page is read one entity past the
            // window, so whether the collection continues is known before any
            // history is read, and only the page's own entities are read.
            let read_window = window.with_lookahead();
            let TemporalEntityIdOutcome {
                entity_ids,
                total: total_count,
            } = self.reader.query_entity_ids(&scope, &compiled, &read_window, count).await?;
            let (entity_ids, reach) = window.settle(entity_ids);
            let entity_ids: Vec<Identifier> = entity_ids
                .into_iter()
                .filter(|entity_id| compiled.contributed.renders_locally(entity_id))
                .collect();

            // One batch read of the temporal catalogue, which owns the entity types
            // (clauses 4.5.1 and 4.5.6) and answers for entities whose current
            // state a clause 5.6.6 deletion has already removed.
            let entity_types_map: HashMap<Identifier, Vec<EntityTypeName>> = self.reader.entity_types(&scope, &entity_ids).await?;

            let dataset_step = DatasetStep::of(&compiled);
            let compactor = self.engine.compactor(context);
            let compactor = EntityCompactor::new(&compactor);
            let mut warnings = Vec::new();
            let (rendered, whole_windows) = if compiled.representation.format.is_aggregated() {
                let aggregated = self
                    .render_aggregated_pages(&scope, entity_ids, &compiled, &entity_types_map, &compactor, &mut warnings)
                    .await?;
                (aggregated, Vec::new())
            } else {
                let (whole_ids, paged_ids): (Vec<Identifier>, Vec<Identifier>) =
                    entity_ids.into_iter().partition(|entity_id| compiled.contributed.reads_whole_window(entity_id));
                let paged = self
                    .render_instance_pages(&scope, paged_ids, &compiled, &entity_types_map, &mut warnings)
                    .await?;
                let whole = self
                    .render_whole_windows(&scope, whole_ids, &compiled, &entity_types_map, &mut warnings)
                    .await?;
                (paged, whole)
            };

            let mut entities = Vec::with_capacity(rendered.len() + whole_windows.len());
            let mut cut = None;
            let mut overall_min: Option<DateTime<Utc>> = None;
            let mut overall_max: Option<DateTime<Utc>> = None;
            for rendered in rendered {
                if !answers(&rendered, dataset_step) {
                    continue;
                }
                cut = cut.or(rendered.cut);
                if let Some((min_t, max_t)) = rendered.time_range {
                    overall_min = Some(overall_min.map_or(min_t, |seen: DateTime<Utc>| seen.min(min_t)));
                    overall_max = Some(overall_max.map_or(max_t, |seen: DateTime<Utc>| seen.max(max_t)));
                }
                entities.push(rendered.history);
            }
            // A whole window is cut once its entity's parts are merged, so it
            // takes no part in the cut or in the instants the range reports.
            entities.extend(
                whole_windows
                    .into_iter()
                    .filter(|rendered| answers(rendered, dataset_step))
                    .map(|rendered| rendered.history),
            );

            let any_truncated = cut.is_some();
            let range = cut.map(|cut| cut.range(compiled.window.interval, overall_min.zip(overall_max)));

            Ok(TemporalQueryResult {
                entities,
                total_count,
                reach,
                any_truncated,
                range,
                warnings,
                page: compiled.instances.page,
                projection: compiled.projection,
            })
        }
        .await;
        timer.finish_result(&result);
        result
    }
}

/// Whether the rendered entity is part of the result set.
///
/// An entity the window selects no history for is simply not part of the
/// result set. One the page's common cut emptied is: its history continues on
/// the next page. So is one only the `datasetId` parameter emptied: clause
/// 5.7.4.4 applies it at step S8, after stage one selected the entity, and S8
/// removes "all Attributes without remaining Attribute instances", never the
/// Entity.
///
/// Clause 5.7.4.5 reduces "every Entity within the payload body" to the
/// members the projection defines. An Entity it would reduce to none of them
/// was never selected (`EntitySelection::projected_members`), so none is left
/// out here, after the page was cut.
fn answers(rendered: &RenderedTemporalEntity, dataset_step: DatasetStep) -> bool {
    rendered.presence == HistoryPresence::Present || dataset_step == DatasetStep::Present
}

impl TemporalService {
    /// Read every instance of the window of each of `entity_ids`, past the
    /// page `compiled` asks for. They are the local parts of the contributed
    /// entities whose merged instances are narrowed, bounded and paged once
    /// every part is merged (clause 5.7.4.4).
    ///
    /// # Errors
    ///
    /// As for [`TemporalService::render_instance_pages`].
    async fn render_whole_windows(
        &self,
        scope: &TemporalReadScope<'_>,
        entity_ids: Vec<Identifier>,
        compiled: &CompiledTemporalQuery,
        entity_types_map: &HashMap<Identifier, Vec<EntityTypeName>>,
        warnings: &mut Vec<TemporalQueryWarning>,
    ) -> Result<Vec<RenderedTemporalEntity>, TemporalError> {
        if entity_ids.is_empty() {
            return Ok(Vec::new());
        }
        // clone: the page's own read keeps the query's page, and the whole windows are read under a copy paging nothing
        let mut whole = compiled.clone();
        whole.instances.page = InstancePage::Whole;
        let whole = whole;
        self.render_instance_pages(scope, entity_ids, &whole, entity_types_map, warnings).await
    }

    /// Read the aggregated page of every entity of the response in one read,
    /// then render each at the response's common cut.
    ///
    /// Clause 6.3.10: one response carries one `Content-Range`, so every
    /// entity of the page is cut at the same period boundary, and an absent
    /// period is one whole range for the whole response (clause 4.5.19.1).
    /// Every page is read before any is rendered, because the cut depends on
    /// all of them.
    ///
    /// # Errors
    ///
    /// Returns the [`TemporalError::TooManyResults`] an entity raised, the
    /// failure of the read itself, or the failure to resolve the names the
    /// request context gives.
    async fn render_aggregated_pages(
        &self,
        scope: &TemporalReadScope<'_>,
        entity_ids: Vec<Identifier>,
        compiled: &CompiledTemporalQuery,
        entity_types_map: &HashMap<Identifier, Vec<EntityTypeName>>,
        compactor: &EntityCompactor<'_>,
        warnings: &mut Vec<TemporalQueryWarning>,
    ) -> Result<Vec<RenderedTemporalEntity>, TemporalError> {
        let mut pages = Vec::with_capacity(entity_ids.len());
        for read in self.read_aggregation_pages(scope, entity_ids, compiled).await? {
            match read {
                Ok(page) => pages.push(page),
                Err(UnreadEntity { entity_id, error }) => {
                    settle_entity(entity_id, Err(*error), warnings)?;
                }
            }
        }
        let cut = settle_aggregation_pages(compiled, &mut pages)?;
        let rendering = AggregationRendering {
            compiled,
            names: compactor.names().await?,
            max_groups: self.limits.aggregation_grouping_max_groups,
            cut,
        };
        let mut rendered = Vec::with_capacity(pages.len());
        for page in pages {
            let entity_types = entity_types_map.get(&page.entity_id).map_or_default(Vec::as_slice);
            // clone: the page is moved into the render, and a failure still names its entity.
            let entity_id = page.entity_id.clone();
            let outcome = render_aggregation_page(page, entity_types, rendering);
            rendered.extend(settle_entity(entity_id, outcome, warnings)?);
        }
        Ok(rendered)
    }

    /// Read the instance page of every entity of the response in one read,
    /// then render each at the response's common cut.
    ///
    /// Clause 6.3.10: one response carries one `Content-Range`, so every
    /// entity of the page is cut at the same instant. Every page is read
    /// before any is rendered, because the cut depends on all of them.
    ///
    /// # Errors
    ///
    /// Returns the [`TemporalError::TooManyResults`] an entity or the common
    /// cut raised, or the failure of the read itself.
    async fn render_instance_pages(
        &self,
        scope: &TemporalReadScope<'_>,
        entity_ids: Vec<Identifier>,
        compiled: &CompiledTemporalQuery,
        entity_types_map: &HashMap<Identifier, Vec<EntityTypeName>>,
        warnings: &mut Vec<TemporalQueryWarning>,
    ) -> Result<Vec<RenderedTemporalEntity>, TemporalError> {
        let mut pages = Vec::with_capacity(entity_ids.len());
        for read in self.read_history_pages(scope, entity_ids, compiled).await? {
            match read {
                Ok(page) => pages.push(page),
                Err(UnreadEntity { entity_id, error }) => {
                    settle_entity(entity_id, Err(*error), warnings)?;
                }
            }
        }
        let cut = self.common_page_cut(compiled, &pages)?;
        let mut rendered = Vec::with_capacity(pages.len());
        for page in pages {
            let entity_types = entity_types_map.get(&page.entity_id).map_or_default(Vec::as_slice);
            // clone: the page is moved into the render, and a failure still names its entity.
            let entity_id = page.entity_id.clone();
            let outcome = render_history_page(page, entity_types, compiled, cut);
            rendered.extend(settle_entity(entity_id, outcome, warnings)?);
        }
        Ok(rendered)
    }
}

/// Settle one entity of a temporal query: its representation, nothing, or a
/// warning.
///
/// One entity that cannot be read or rendered degrades the result set rather
/// than the query (the rest of the matches still answer), except for a
/// `TooManyResults` (403), which refuses the whole operation rather than
/// dropping one entity from it.
///
/// # Errors
///
/// Returns the [`TemporalError::TooManyResults`] the entity raised.
fn settle_entity(
    entity_id: Identifier,
    outcome: Result<RenderedTemporalEntity, TemporalError>,
    warnings: &mut Vec<TemporalQueryWarning>,
) -> Result<Option<RenderedTemporalEntity>, TemporalError> {
    match outcome {
        Ok(rendered) => Ok(Some(rendered)),
        Err(error @ TemporalError::TooManyResults { .. }) => Err(error),
        Err(error) => {
            tracing::warn!(error = %error, entity_id = %entity_id, "Failed to render the temporal evolution of an entity");
            warnings.push(TemporalQueryWarning {
                entity_id, // moved: the warning owns the identifier this entity finished with
                reason: TemporalQueryWarningReason::Unrenderable(error),
            });
            Ok(None)
        }
    }
}
