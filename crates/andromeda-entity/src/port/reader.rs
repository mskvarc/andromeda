//! Entity payload, version, and system-attribute reads.

use crate::{entity_snapshot::EntitySnapshot, error::EntityError};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use futures::future::BoxFuture;
use serde_json::Value;
use std::collections::HashMap;

/// Reads entity payloads and the snapshots needed by optimistic writes.
pub trait EntityReader: Send + Sync + 'static {
    /// Read one expanded entity payload.
    fn get<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        id: &'life2 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<Value>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// Read expanded entity payloads keyed by identifier.
    fn get_batch<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        ids: &'life2 [Identifier],
    ) -> BoxFuture<'async_trait, Result<HashMap<Identifier, Value>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// Read one expanded payload with its optimistic version token.
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
    /// Read one entity's clause 4.8 system attributes.
    fn get_sys_attrs<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        id: &'life2 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<SysAttrsSnapshot>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
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
