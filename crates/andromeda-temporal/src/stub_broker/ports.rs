//! The ports of the stubbed broker: what each answers from its graph, and what
//! it refuses because no test reaches it.

use crate::{
    error::TemporalError,
    observed_instance::ObservedInstance,
    outcome::TemporalEntityIdOutcome,
    port::{
        history_reader::{
            AggregationPushdown,
            HistoryVisibility,
            RecordedAttributeShape,
            ShapeHolders,
            TemporalAggregationOutcome,
            TemporalHistoryObservation,
            TemporalHistoryOutcome,
            TemporalHistoryReader,
            TemporalReaderCapabilities,
        },
        in_effect_reads::InEffectReads,
        mutation_barrier::TemporalMutationBarrier,
        mutator::{TemporalDeleteScope, TemporalMutator},
        read_scope::{ScopeReach, TemporalReadScope, UnrecordedHistory},
    },
    stub_broker::{CurrentState, StubBroker},
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_replay::CommitReplay,
    committer::{CommitFuture, ContextCommitter},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    port::{reader::EntityReader, retrieved_entity::RetrievedEntity},
};
use andromeda_ngsi_ld_entity::{entity_state::EntityState, entity_types::EntityTypes};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    entity_type_name::EntityTypeName,
    identifier::Identifier,
    instance_identity::InstanceIdentity,
    time::system_instant::SystemInstant,
};
use andromeda_ngsi_ld_model::{
    notification::SysAttrsSnapshot,
    temporal::{TemporalAggregationRow, TemporalHistoryRow},
};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    page_window::PageWindow,
    projection::returned_attributes::ReturnedAttributes,
    split_filter::LeafTruths,
    temporal::{compiled::CompiledTemporalQuery, history_delete::TemporalHistoryDeleteScope},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

fn unsupported(method: &str) -> TemporalError {
    panic!("StubBroker::{method} is not exercised by the traversal harness")
}

#[async_trait]
impl TemporalHistoryReader for StubBroker {
    fn capabilities(&self) -> TemporalReaderCapabilities {
        TemporalReaderCapabilities {
            aggregation: AggregationPushdown::OnTheFly,
            history_visibility: HistoryVisibility::Immediate,
        }
    }

    async fn read_scope<'a>(&self, target: &'a ContextTarget, _reach: ScopeReach<'_>) -> Result<TemporalReadScope<'a>, TemporalError> {
        Ok(TemporalReadScope::new(target, UnrecordedHistory::Absent))
    }

    async fn entity_exists(&self, _scope: &TemporalReadScope<'_>, entity_id: &Identifier) -> Result<bool, TemporalError> {
        Ok(self.links.contains_key(entity_id))
    }

    async fn entity_types(&self, _scope: &TemporalReadScope<'_>, entity_ids: &[Identifier]) -> Result<HashMap<Identifier, Vec<EntityTypeName>>, TemporalError> {
        Ok(entity_ids
            .iter()
            .filter(|entity_id| self.links.contains_key(*entity_id))
            .map(|entity_id| {
                (
                    entity_id.clone(),
                    vec!["https://example.com/Thing".parse::<EntityTypeName>().expect("valid type IRI")],
                )
            }) // clone: the index owns its validated entity URI.
            .collect())
    }

    async fn query_entity_ids(
        &self,
        _scope: &TemporalReadScope<'_>,
        compiled: &CompiledTemporalQuery,
        _window: &PageWindow,
        _count: CountPolicy,
    ) -> Result<TemporalEntityIdOutcome, TemporalError> {
        self.listed_selections
            .lock()
            .expect("the recording lock is never poisoned")
            .push(compiled.entities.clone()); // clone: the recording outlives the compiled query
        Ok(TemporalEntityIdOutcome {
            entity_ids: Vec::new(),
            total: None,
        })
    }

    async fn filter_leaf_truths(
        &self,
        _scope: &TemporalReadScope<'_>,
        _compiled: &CompiledTemporalQuery,
        entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, LeafTruths>, TemporalError> {
        Ok(self.leaf_truths_of(entity_ids))
    }

    async fn in_effect_instances(
        &self,
        _scope: &TemporalReadScope<'_>,
        _compiled: &CompiledTemporalQuery,
        entity_ids: &[Identifier],
        reads: InEffectReads<'_>,
    ) -> Result<HashMap<Identifier, Vec<TemporalHistoryRow>>, TemporalError> {
        Ok(self.in_effect_of(entity_ids, reads))
    }

    async fn window_relationship_instances(
        &self,
        _scope: &TemporalReadScope<'_>,
        _compiled: &CompiledTemporalQuery,
        entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, Vec<TemporalHistoryRow>>, TemporalError> {
        Ok(self.window_relationships_of(entity_ids))
    }

    async fn query_entity_history(
        &self,
        _scope: &TemporalReadScope<'_>,
        entity_id: &Identifier,
        _compiled: &CompiledTemporalQuery,
    ) -> Result<TemporalHistoryOutcome, TemporalError> {
        Ok(TemporalHistoryOutcome {
            observations: self
                .rows(entity_id)
                .into_iter()
                .map(|row| TemporalHistoryObservation { row, identity: None })
                .collect(),
        })
    }

    async fn query_entity_aggregation(
        &self,
        _scope: &TemporalReadScope<'_>,
        _entity_id: &Identifier,
        _compiled: &CompiledTemporalQuery,
    ) -> Result<TemporalAggregationOutcome, TemporalError> {
        Err(unsupported("query_entity_aggregation"))
    }
    async fn query_merged_entity_aggregation(
        &self,
        _scope: &TemporalReadScope<'_>,
        _entity_id: &Identifier,
        _entity_types: &[EntityTypeName],
        _compiled: &CompiledTemporalQuery,
        answered: &[TemporalHistoryRow],
    ) -> Result<Vec<TemporalAggregationRow>, TemporalError> {
        self.merged_answers
            .lock()
            .expect("the recording lock is never poisoned")
            .extend_from_slice(answered);
        Ok(self.aggregated.clone()) // clone: every merged aggregation is answered with the same rows
    }

    async fn recorded_attribute_shapes(
        &self,
        _scope: &TemporalReadScope<'_>,
        _attributes: &[AttributeName],
    ) -> Result<HashMap<AttributeName, HashSet<RecordedAttributeShape>>, TemporalError> {
        Ok(HashMap::new())
    }

    async fn held_attribute_shapes(
        &self,
        _scope: &TemporalReadScope<'_>,
        _holders: ShapeHolders<'_>,
        _compiled: &CompiledTemporalQuery,
        _shapes: &HashMap<AttributeName, HashSet<RecordedAttributeShape>>,
    ) -> Result<HashMap<AttributeName, HashSet<RecordedAttributeShape>>, TemporalError> {
        Ok(HashMap::new())
    }
}

/// The current state of one known entity: a `Thing` holding no attribute.
fn thing(id: &Identifier) -> Arc<EntityState> {
    let thing = "https://example.com/Thing".parse::<EntityTypeName>().expect("valid type IRI");
    Arc::new(EntityState::new(id.clone(), EntityTypes::one(thing))) // clone: the state owns its identifier
}

#[async_trait]
impl EntityReader for StubBroker {
    async fn get(&self, _target: &ContextTarget, id: &Identifier) -> Result<Option<Arc<EntityState>>, EntityError> {
        Ok(self.links.contains_key(id).then(|| thing(id)))
    }

    async fn get_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, RetrievedEntity>, EntityError> {
        Ok(ids
            .iter()
            .filter(|id| matches!(self.current_state, CurrentState::Held) && self.links.contains_key(*id))
            .map(|id| {
                let retrieved = RetrievedEntity {
                    state: thing(id),
                    sys_attrs: SysAttrsSnapshot {
                        created_at: DateTime::<Utc>::UNIX_EPOCH,
                        modified_at: DateTime::<Utc>::UNIX_EPOCH,
                        expires_at: None,
                    },
                };
                (id.clone(), retrieved) // clone: the port hands back owned keys
            })
            .collect())
    }

    async fn get_with_sys_attrs(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _returned: &ReturnedAttributes<'_>,
    ) -> Result<Option<RetrievedEntity>, EntityError> {
        panic!("get_with_sys_attrs is not exercised by the traversal harness")
    }

    async fn get_snapshot(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<EntitySnapshot>, EntityError> {
        panic!("get_snapshot is not exercised by the traversal harness")
    }

    async fn get_snapshot_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, EntitySnapshot>, EntityError> {
        panic!("get_snapshot_batch is not exercised by the traversal harness")
    }

    async fn get_sys_attrs_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, SysAttrsSnapshot>, EntityError> {
        panic!("get_sys_attrs_batch is not exercised by the traversal harness")
    }
}

#[async_trait]
impl TemporalMutationBarrier for StubBroker {
    async fn synchronize(&self, _target: &ContextTarget, _entity_id: &Identifier) -> Result<(), TemporalError> {
        Err(unsupported("synchronize"))
    }
}

#[async_trait]
impl TemporalMutator for StubBroker {
    async fn entity_exists(&self, _target: &ContextTarget, _entity_id: &Identifier) -> Result<bool, TemporalError> {
        Err(unsupported("entity_exists"))
    }

    async fn attribute_exists(&self, _target: &ContextTarget, _entity_id: &Identifier, _attribute_name: &AttributeName) -> Result<bool, TemporalError> {
        Err(unsupported("attribute_exists"))
    }

    async fn attribute_names_for_entity(&self, _target: &ContextTarget, _entity_id: &Identifier) -> Result<Vec<AttributeName>, TemporalError> {
        Err(unsupported("attribute_names_for_entity"))
    }

    async fn delete_all_for_entity(&self, _target: &ContextTarget, _entity_id: &Identifier) -> Result<u64, TemporalError> {
        Err(unsupported("delete_all_for_entity"))
    }

    async fn delete_attribute(
        &self,
        _target: &ContextTarget,
        _entity_id: &Identifier,
        _attribute_name: &AttributeName,
        _dataset_id: Option<&Identifier>,
        _scope: TemporalDeleteScope,
    ) -> Result<u64, TemporalError> {
        Err(unsupported("delete_attribute"))
    }

    async fn instance_exists(
        &self,
        _target: &ContextTarget,
        _instance: &InstanceIdentity,
        _entity_id: &Identifier,
        _attribute_name: &AttributeName,
    ) -> Result<bool, TemporalError> {
        Err(unsupported("instance_exists"))
    }

    async fn update_instance(
        &self,
        _target: &ContextTarget,
        _instance: &InstanceIdentity,
        _entity_id: &Identifier,
        _attribute_name: &AttributeName,
        _replacement: &ObservedInstance,
    ) -> Result<(), TemporalError> {
        Err(unsupported("update_instance"))
    }

    async fn delete_instance(
        &self,
        _target: &ContextTarget,
        _instance: &InstanceIdentity,
        _entity_id: &Identifier,
        _attribute_name: &AttributeName,
    ) -> Result<(), TemporalError> {
        Err(unsupported("delete_instance"))
    }

    async fn count_history_instances(
        &self,
        _target: &ContextTarget,
        _entity_ids: &[Identifier],
        _scope: &TemporalHistoryDeleteScope,
    ) -> Result<u64, TemporalError> {
        Err(unsupported("count_history_instances"))
    }

    async fn delete_history_instances(
        &self,
        _target: &ContextTarget,
        _entity_ids: &[Identifier],
        _scope: &TemporalHistoryDeleteScope,
    ) -> Result<u64, TemporalError> {
        Err(unsupported("delete_history_instances"))
    }
}

impl ContextCommitter for StubBroker {
    fn commit(&self, _target: &ContextTarget, _accepted_at: SystemInstant, _writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        Box::pin(async { panic!("commit is not exercised by the traversal harness") })
    }

    fn commit_enqueued(&self, _accepted_at: SystemInstant, _writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        Box::pin(async { panic!("commit_enqueued is not exercised by the traversal harness") })
    }

    fn commit_replayed(&self, _target: &ContextTarget, _replay: CommitReplay, _writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        Box::pin(async { panic!("commit_replayed is not exercised by the traversal harness") })
    }
}
