//! Execution and persistence of individual snapshot queries.

use crate::executor::{
    SnapshotExecutor,
    status::{failure_detail, success_detail},
    translation::{entity_parameters, temporal_parameters},
};
use andromeda_context_commit::{atomic_write::AtomicWrite, entity_mutation::EntityMutation, entity_record::EntityRecord};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::EntityQueryService;
use andromeda_federation::{
    entity_result::collect_warnings,
    forwarded_query::client_query::ClientQuery,
    operation::ForwardingOperation,
    request::{CsrModeFilter, EntityMapInUse, ForwardQueryRequest, ForwardTemporalQueryRequest, QueryDelivery, SourcePage, TriggeringRequest},
    temporal_result::merge_temporal_query_results,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{query::body::Query, snapshot::ExecutionResultDetails};
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};
use tracing::{error, warn};

impl SnapshotExecutor {
    pub(crate) async fn run_entity_query(
        &self,
        target: &ContextTarget,
        snapshot_id: &Uri,
        query: Query,
        index: usize,
        context: &ContextSpec,
        materialized_entities: &mut HashSet<Uri>,
    ) -> ExecutionResultDetails {
        let mut params = match entity_parameters(query) {
            Ok(params) => params,
            Err(error) => return failure_detail(error.to_string()),
        };
        if let Err(error) = params.validate() {
            return failure_detail(format!("Snapshot query translation failed validation: {error}"));
        }

        let mut offset = 0_u32;
        let mut captured = HashSet::new();
        loop {
            params.limit = Some(self.capture_page_size);
            params.offset = Some(offset);
            params.count = Some(true);
            let (mut entities, _) = match self.sources.entity_query.query(&self.sources.live_target, &params, context).await {
                Ok(result) => result,
                Err(error) => {
                    warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Snapshot entity query failed");
                    return failure_detail(error.to_string());
                }
            };
            let local_page_len = entities.len();
            let forwarded_result = self
                .sources
                .forwarding
                .forward_query(
                    &self.sources.live_target,
                    ForwardQueryRequest {
                        client_query: ClientQuery::EntityQuery(&params),
                        // A Snapshot materialises its members through the GET query
                        // binding (clause 5.5.15).
                        delivery: QueryDelivery::Query,
                        split: self.sources.forwarding.split_entities(params.split_entities),
                        context,
                        triggering: TriggeringRequest::default(),
                        csf_expr: None,
                        mode_filter: CsrModeFilter::All,
                        entity_map: EntityMapInUse::None,
                        source_page: Some(SourcePage {
                            limit: self.capture_page_size,
                            offset,
                        }),
                    },
                )
                .await;
            let forwarded = match forwarded_result {
                Ok(forwarded) => forwarded.results,
                Err(error) => {
                    warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Snapshot CSR forwarding failed");
                    if local_page_len == 0 && captured.is_empty() {
                        return failure_detail(format!("No valid entity result was available because distributed execution failed: {error}"));
                    }
                    Vec::new()
                }
            };
            let forwarding_warnings = collect_warnings(&forwarded);
            for warning in &forwarding_warnings {
                warn!(snapshot_id = %snapshot_id, query_index = index, warning = %warning, "Snapshot entity query retained a distributed warning");
            }
            let remote_page_full = forwarded.iter().any(|result| result.entities.len() >= self.capture_page_size as usize);
            let remote_page_len = forwarded.iter().map(|result| result.entities.len()).sum::<usize>();
            if local_page_len == 0 && remote_page_len == 0 && captured.is_empty() && !forwarding_warnings.is_empty() {
                return failure_detail("No valid entity result was available because every matching distributed source failed");
            }
            if !forwarded.is_empty() {
                entities = EntityQueryService::merge_with_forwarded(entities, forwarded, None);
            }
            let mut pairs = Vec::with_capacity(entities.len());
            for entity in entities {
                let identifier = entity.id.clone(); // clone: the atomic write and result accounting independently own the identifier.
                let compact = match serde_json::to_value(entity) {
                    Ok(compact) => compact,
                    Err(error) => return failure_detail(format!("Failed to serialize captured entity: {error}")),
                };
                let expanded = match self.sources.entity_query.engine().expand(compact, context).await {
                    Ok(expanded) => expanded,
                    Err(error) => return failure_detail(format!("Failed to expand captured entity: {error}")),
                };
                pairs.push((identifier, expanded));
            }
            let new_pairs = pairs.iter().filter(|(id, _)| !materialized_entities.contains(id)).cloned().collect::<Vec<_>>();
            let detail = self.persist_current_results(target, snapshot_id, index, &new_pairs).await;
            if detail.problem_details.is_some() {
                return detail;
            }
            captured.extend(pairs.iter().map(|(id, _)| id.clone()));
            materialized_entities.extend(new_pairs.into_iter().map(|(id, _)| id));
            if local_page_len < self.capture_page_size as usize && !remote_page_full {
                break;
            }
            offset = match offset.checked_add(self.capture_page_size) {
                Some(offset) => offset,
                None => return failure_detail("Snapshot entity pagination offset overflowed"),
            };
        }
        success_detail(captured.len())
    }

    pub(crate) async fn run_temporal_query(
        &self,
        target: &ContextTarget,
        snapshot_id: &Uri,
        query: Query,
        index: usize,
        context: &ContextSpec,
    ) -> ExecutionResultDetails {
        let mut params = match temporal_parameters(query) {
            Ok(params) => params,
            Err(error) => return failure_detail(error.to_string()),
        };
        let mut offset = 0_u32;
        let mut captured = HashSet::new();
        loop {
            params.limit = Some(self.capture_page_size);
            params.offset = Some(offset);
            params.count = Some(true);
            let result = match self.sources.temporal.query(&self.sources.live_target, &params, context).await {
                Ok(result) => result,
                Err(error) => {
                    warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Snapshot temporal query failed");
                    return failure_detail(error.to_string());
                }
            };
            for warning in &result.warnings {
                warn!(snapshot_id = %snapshot_id, query_index = index, entity_id = %warning.entity_id, warning = %warning.message, "Snapshot temporal query retained usable results with a warning");
            }
            let local_page_len = result.entities.len();
            let forwarded = match self
                .sources
                .forwarding
                .forward_temporal_query(
                    &self.sources.live_target,
                    ForwardTemporalQueryRequest {
                        client_query: &params,
                        split: self.sources.forwarding.split_entities(params.split_entities),
                        context,
                        operation: ForwardingOperation::QueryTemporalEntity,
                        triggering: TriggeringRequest::default(),
                        csf_expr: None,
                        mode_filter: CsrModeFilter::All,
                        entity_map: EntityMapInUse::None,
                        source_page: Some(SourcePage {
                            limit: self.capture_page_size,
                            offset,
                        }),
                    },
                )
                .await
            {
                Ok(forwarded) => forwarded.results,
                Err(error) => {
                    warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Snapshot temporal CSR forwarding failed");
                    Vec::new()
                }
            };
            let remote_page_full = forwarded.iter().any(|source| source.entities.len() >= self.capture_page_size as usize);
            let entities = merge_temporal_query_results(result.entities, forwarded);
            let pairs = entities
                .into_iter()
                .filter_map(|value| {
                    let raw = value.get("id").and_then(Value::as_str)?;
                    match raw.parse::<Uri>() {
                        Ok(id) => Some((id, value)),
                        Err(error) => {
                            warn!(snapshot_id = %snapshot_id, query_index = index, entity_id = raw, error = %error, "Temporal snapshot result carries an invalid entity URI");
                            None
                        }
                    }
                })
                .collect::<Vec<_>>();
            let detail = self.persist_temporal_results(target, snapshot_id, index, &pairs).await;
            if detail.problem_details.is_some() {
                return detail;
            }
            captured.extend(pairs.into_iter().map(|(id, _)| id));
            if local_page_len < self.capture_page_size as usize && !remote_page_full {
                break;
            }
            offset = match offset.checked_add(self.capture_page_size) {
                Some(offset) => offset,
                None => return failure_detail("Snapshot temporal pagination offset overflowed"),
            };
        }
        success_detail(captured.len())
    }

    async fn persist_current_results(&self, target: &ContextTarget, snapshot_id: &Uri, index: usize, pairs: &[(Uri, Value)]) -> ExecutionResultDetails {
        if pairs.is_empty() {
            return success_detail(0);
        }
        let writes = pairs
            .iter()
            .map(|(entity_id, data)| {
                AtomicWrite::builder()
                    .entity_id(Arc::new(entity_id.clone())) // clone: the write owns its validated entity identity.
                    .entity(EntityMutation::Insert(EntityRecord::from_expanded(Arc::new(data.clone())))) // clone: captured state is owned by the branch write.
                    .build()
            })
            .collect();
        if let Err(error) = self.writes.commit(target, writes).await {
            error!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Failed to materialize current Snapshot results");
            return failure_detail(format!("Failed to store results: {error}"));
        }
        success_detail(pairs.len())
    }

    async fn persist_temporal_results(&self, target: &ContextTarget, snapshot_id: &Uri, index: usize, pairs: &[(Uri, Value)]) -> ExecutionResultDetails {
        for (_, data) in pairs {
            if let Err(error) = self.sources.temporal.create_or_update(target, data.clone(), &ContextSpec::core_only()).await {
                error!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Failed to materialize temporal Snapshot results");
                return failure_detail(format!("Failed to store temporal results: {error}"));
            }
        }
        success_detail(pairs.len())
    }
}
