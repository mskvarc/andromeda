//! The entities this broker holds for a re-assembly test, read through the
//! entity ports.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    port::{query::EntityQueryStore, reader::EntityReader, retrieved_entity::RetrievedEntity, stored_entry::StoredEntry},
    query_outcome::QueryOutcome,
    storage_profile::StorageProfile,
};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity::compiled::CompiledEntityQuery,
    page_window::PageWindow,
    projection::returned_attributes::ReturnedAttributes,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{collections::HashMap, sync::Arc};

/// The entities the broker under test holds, answered by identifier; a query
/// is a store failure, because re-assembly never asks one.
pub(crate) struct HeldEntities {
    /// The held entities, by identifier.
    states: HashMap<Identifier, Arc<EntityState>>,
}

impl HeldEntities {
    /// A store holding `states`.
    pub(crate) fn new(states: Vec<EntityState>) -> HeldEntities {
        HeldEntities {
            states: states.into_iter().map(|state| (state.id().clone(), Arc::new(state))).collect(),
        }
    }

    /// The system times every held entity carries.
    fn system_times() -> SysAttrsSnapshot {
        let instant: DateTime<Utc> = DateTime::<Utc>::MIN_UTC;
        SysAttrsSnapshot {
            created_at: instant,
            modified_at: instant,
            expires_at: None,
        }
    }
}

#[async_trait]
impl EntityReader for HeldEntities {
    async fn get(&self, _target: &ContextTarget, id: &Identifier) -> Result<Option<Arc<EntityState>>, EntityError> {
        Ok(self.states.get(id).map(Arc::clone))
    }

    async fn get_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, RetrievedEntity>, EntityError> {
        Ok(ids
            .iter()
            .filter_map(|id| {
                self.states.get(id).map(|state| {
                    (
                        id.clone(),
                        RetrievedEntity {
                            state: Arc::clone(state),
                            sys_attrs: HeldEntities::system_times(),
                        },
                    )
                })
            })
            .collect())
    }

    async fn get_with_sys_attrs(
        &self,
        _target: &ContextTarget,
        id: &Identifier,
        _returned: &ReturnedAttributes<'_>,
    ) -> Result<Option<RetrievedEntity>, EntityError> {
        Ok(self.states.get(id).map(|state| RetrievedEntity {
            state: Arc::clone(state),
            sys_attrs: HeldEntities::system_times(),
        }))
    }

    async fn get_snapshot(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<EntitySnapshot>, EntityError> {
        Ok(None)
    }

    async fn get_snapshot_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, EntitySnapshot>, EntityError> {
        Ok(HashMap::new())
    }

    async fn get_sys_attrs_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, SysAttrsSnapshot>, EntityError> {
        Ok(ids
            .iter()
            .filter(|id| self.states.contains_key(*id))
            .map(|id| (id.clone(), HeldEntities::system_times()))
            .collect())
    }
}

#[async_trait]
impl EntityQueryStore for HeldEntities {
    fn profile(&self) -> &StorageProfile {
        &StorageProfile::POSTGIS_LTREE
    }

    async fn query_compiled(
        &self,
        _target: &ContextTarget,
        _compiled: &CompiledEntityQuery,
        _window: &PageWindow,
        _count: CountPolicy,
    ) -> Result<QueryOutcome<StoredEntry<RetrievedEntity>>, EntityError> {
        Err(EntityError::StoreUnavailable)
    }

    async fn query_ids(&self, _target: &ContextTarget, _compiled: &CompiledEntityQuery, _window: &PageWindow) -> Result<QueryOutcome<Identifier>, EntityError> {
        Err(EntityError::StoreUnavailable)
    }
}
