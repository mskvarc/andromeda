//! Entity state, version, and system-attribute reads.

use crate::{entity_snapshot::EntitySnapshot, error::EntityError, port::retrieved_entity::RetrievedEntity};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_query::projection::returned_attributes::ReturnedAttributes;
use futures::future::BoxFuture;
use std::{collections::HashMap, sync::Arc};

/// Reads entity states and the snapshots needed by optimistic writes.
pub trait EntityReader: Send + Sync + 'static {
    /// Read one stored entity state.
    fn get<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        id: &'life2 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<Arc<EntityState>>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// Read stored entity states keyed by identifier, each with its
    /// entity-level clause 4.8 system times, in one read.
    fn get_batch<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        ids: &'life2 [Identifier],
    ) -> BoxFuture<'async_trait, Result<HashMap<Identifier, RetrievedEntity>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// Read one stored state with its optimistic version token.
    fn get_snapshot<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        id: &'life2 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<EntitySnapshot>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// Read versioned snapshots keyed by identifier.
    fn get_snapshot_batch<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        ids: &'life2 [Identifier],
    ) -> BoxFuture<'async_trait, Result<HashMap<Identifier, EntitySnapshot>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// Read one stored state together with its entity-level clause 4.8
    /// system times, in one read (clause 5.6.3 retrieval with `sysAttrs`).
    ///
    /// The state holds at least the Attributes `returned` lets the response
    /// hold; a reader may leave every other Attribute out, so the caller
    /// still narrows what it is handed.
    fn get_with_sys_attrs<'life0, 'life1, 'life2, 'life3, 'life4, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        id: &'life2 Identifier,
        returned: &'life3 ReturnedAttributes<'life4>,
    ) -> BoxFuture<'async_trait, Result<Option<RetrievedEntity>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        'life4: 'async_trait,
        Self: 'async_trait;
    /// Read system attributes keyed by entity identifier.
    fn get_sys_attrs_batch<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        ids: &'life2 [Identifier],
    ) -> BoxFuture<'async_trait, Result<HashMap<Identifier, SysAttrsSnapshot>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
}
