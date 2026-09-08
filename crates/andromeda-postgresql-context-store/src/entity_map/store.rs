//! `EntityMapStore` trait implementation for `EntityMapRepository`.

use crate::entity_map::{
    entries::{insert_children, read_entries, read_links, upsert_link},
    lifecycle::{delete_expired, delete_live, delete_map_row, insert_map_row, read_live_control, update_expiry},
    mapper::rows_to_entity_map,
    persistence_error,
    repo::EntityMapRepository,
};
use andromeda_entity_map::store::{EntityMapMutation, EntityMapStore, EntityMapStoreFuture};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::entity_map::EntityMap;
use chrono::{DateTime, Utc};

impl EntityMapStore for EntityMapRepository {
    fn insert<'a>(&'a self, map: &'a EntityMap) -> EntityMapStoreFuture<'a, ()> {
        Box::pin(async move {
            // The control record and its children land in one transaction, so
            // a follow-up page on another instance never sees a map without
            // its entries.
            let mut transaction = self.pool().begin().await.map_err(persistence_error)?;
            delete_map_row(&mut transaction, &map.id).await?;
            insert_map_row(&mut transaction, map).await?;
            insert_children(&mut transaction, map).await?;
            transaction.commit().await.map_err(persistence_error)?;
            Ok(())
        })
    }

    fn get<'a>(&'a self, identifier: &'a Uri) -> EntityMapStoreFuture<'a, Option<EntityMap>> {
        Box::pin(async move {
            let Some(control) = read_live_control(self.pool(), identifier).await? else {
                return Ok(None);
            };
            let entries = read_entries(self.pool(), identifier).await?;
            let links = read_links(self.pool(), identifier).await?;
            let map = rows_to_entity_map(identifier.clone(), &control, entries, links).map_err(persistence_error)?; // clone: the map owns its identifier
            Ok(Some(map))
        })
    }

    fn update<'a>(&'a self, identifier: &'a Uri, expires_at: DateTime<Utc>) -> EntityMapStoreFuture<'a, EntityMapMutation> {
        Box::pin(async move { update_expiry(self.pool(), identifier, expires_at).await })
    }

    fn link<'a>(&'a self, identifier: &'a Uri, registration: &'a Uri, linked_map: &'a Uri) -> EntityMapStoreFuture<'a, EntityMapMutation> {
        Box::pin(async move { upsert_link(self.pool(), identifier, registration, linked_map).await })
    }

    fn delete<'a>(&'a self, identifier: &'a Uri) -> EntityMapStoreFuture<'a, EntityMapMutation> {
        Box::pin(async move { delete_live(self.pool(), identifier).await })
    }

    fn delete_expired(&self, cutoff: DateTime<Utc>) -> EntityMapStoreFuture<'_, u64> {
        Box::pin(async move { delete_expired(self.pool(), cutoff).await })
    }
}
