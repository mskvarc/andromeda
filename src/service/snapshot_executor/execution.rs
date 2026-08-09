//! Execution and persistence of individual snapshot queries.

use crate::service::{
    entity::query::EntityQueryService,
    snapshot::store::SnapshotEntityInput,
    snapshot_executor::{
        SnapshotExecutor,
        status::{failure_detail, success_detail},
        translation::{entity_parameters, temporal_parameters},
    },
};
use andromeda_federation::{
    operation::ForwardingOperation,
    request::{CsrModeFilter, ForwardQueryRequest},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{query::body::Query, snapshot::ExecutionResultDetails};
use serde_json::Value;
use tracing::{error, warn};

impl SnapshotExecutor {
    pub(crate) async fn run_entity_query(&self, snapshot_id: &Uri, query: Query, index: usize) -> ExecutionResultDetails {
        let params = match entity_parameters(query) {
            Ok(params) => params,
            Err(error) => return failure_detail(error.to_string()),
        };
        if let Err(error) = params.validate() {
            return failure_detail(format!("Snapshot query translation failed validation: {error}"));
        }

        let context = ContextSpec::core_only();
        let (mut entities, _) = match self.entity_query.query(&params, &context).await {
            Ok(result) => result,
            Err(error) => {
                warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Snapshot entity query failed");
                return failure_detail(error.to_string());
            }
        };
        let entity_ids = params.id.clone();
        let entity_type = params.entity_type.as_ref();
        let forwarded = self
            .forwarding
            .forward_query(ForwardQueryRequest {
                entity_type_compact: entity_type,
                entity_ids: entity_ids.as_deref(),
                context_url: None,
                context: &context,
                operation: ForwardingOperation::QueryBatch,
                incoming_via: None,
                csf_expr: None,
                mode_filter: CsrModeFilter::All,
            })
            .await
            .unwrap_or_else(|error| {
                warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Snapshot CSR forwarding failed");
                Vec::new()
            });
        if !forwarded.is_empty() {
            entities = EntityQueryService::merge_with_forwarded(entities, forwarded, None);
        }

        let pairs: Vec<(Uri, Value)> = entities
            .into_iter()
            .filter_map(|entity| match serde_json::to_value(&entity) {
                Ok(value) => Some((entity.id, value)),
                Err(error) => {
                    warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, entity_id = %entity.id, "Failed to serialize snapshot entity");
                    None
                }
            })
            .collect();
        self.persist_results(snapshot_id, index, &pairs, SnapshotResultKind::Entity).await
    }

    pub(crate) async fn run_temporal_query(&self, snapshot_id: &Uri, query: Query, index: usize) -> ExecutionResultDetails {
        let params = match temporal_parameters(query) {
            Ok(params) => params,
            Err(error) => return failure_detail(error.to_string()),
        };
        let context = ContextSpec::core_only();
        let result = match self.temporal.query(&params, &context).await {
            Ok(result) => result,
            Err(error) => {
                warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Snapshot temporal query failed");
                return failure_detail(error.to_string());
            }
        };
        let pairs = result
            .entities
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
        self.persist_results(snapshot_id, index, &pairs, SnapshotResultKind::Temporal).await
    }

    async fn persist_results(&self, snapshot_id: &Uri, index: usize, pairs: &[(Uri, Value)], kind: SnapshotResultKind) -> ExecutionResultDetails {
        let inputs = pairs
            .iter()
            .map(|(entity_id, data)| SnapshotEntityInput { entity_id, data })
            .collect::<Vec<_>>();
        let result = match kind {
            SnapshotResultKind::Entity => self.snapshot_repo.insert_entities(snapshot_id, &inputs).await,
            SnapshotResultKind::Temporal => self.snapshot_repo.insert_temporal_entities(snapshot_id, &inputs).await,
        };
        if !inputs.is_empty()
            && let Err(error) = result
        {
            error!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Failed to store snapshot query results");
            return failure_detail(format!("Failed to store results: {error}"));
        }
        success_detail(inputs.len())
    }
}

enum SnapshotResultKind {
    Entity,
    Temporal,
}
