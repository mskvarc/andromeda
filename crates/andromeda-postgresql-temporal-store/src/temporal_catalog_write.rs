//! Establishing the temporal catalogue rows one chunk of writes' history hangs
//! off.
//!
//! The catalogue is two relations, each written once per chunk rather than once
//! per entity. `temporal_entities` names the entity whose Temporal Evolution is
//! being recorded and carries the narrow key everything else references;
//! `temporal_attribute_streams` names one attribute of that entity under one
//! `datasetId` and hands out the narrow key every history row is stored against.
//! This module is the coordinator: it resolves every chunk entity's narrow key
//! first — because a stream references the entity it belongs to — then resolves
//! every stream against those keys. Each half lives in its own module
//! ([`upsert_temporal_entities_batch`](TemporalRepository::upsert_temporal_entities_batch)
//! and
//! [`resolve_stream_catalog_batch`](TemporalRepository::resolve_stream_catalog_batch)),
//! and neither references `entities`: clause 5.6.6 leaves temporal evolution
//! standing when current state is deleted.

use crate::{
    error::TemporalStoreError,
    repo::TemporalRepository,
    temporal_stream::{TemporalCatalogUpsert, TemporalStreamKey},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_codes::{stream_key::StreamKey, temporal_entity_key::TemporalEntityKey};
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};
use std::collections::HashMap;

/// The narrow keys one chunk of writes resolved its catalogue to.
///
/// The entity keys bridge each chunk entity's identifier to the narrow key its
/// temporal-entity row carries; the stream keys are keyed by that narrow entity
/// key beside the grain, so a caller holding an entity's key resolves each of
/// its grains to the stream every history row is stored against.
pub(crate) struct TemporalCatalog {
    /// Narrow key each chunk entity resolved to.
    pub(crate) entity_keys: HashMap<Identifier, TemporalEntityKey>,
    /// Stream each requested grain of each entity resolved to.
    pub(crate) streams: HashMap<(TemporalEntityKey, TemporalStreamKey), StreamKey>,
}

impl TemporalRepository {
    /// Establish the catalogue rows a whole chunk's history rows are stored
    /// against, in a fixed number of statements regardless of the chunk's size.
    ///
    /// Every entity's narrow key is resolved first: the entities whose write may
    /// bring an evolution into being are upserted and their keys harvested, the
    /// rest looked up, and a typeless observation with no evolution to extend
    /// fails the whole chunk while a deletion with none simply resolves to no
    /// key. The streams of every entity are then created against those keys --
    /// without conflict arbitration for an evolution the chunk itself created --
    /// and only the streams that already existed are read back, so an entity
    /// the entity half passed over contributes none.
    /// Returns both maps so the caller can resolve each of its rows' grains to
    /// the stream it belongs to.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::NameLookup`] when a name dictionary cannot
    /// resolve a name, [`TemporalStoreError::NotFound`] when a typeless
    /// observation has no temporal evolution to extend,
    /// [`TemporalStoreError::Invariant`] when the catalogue does not return a
    /// requested grain, and [`TemporalStoreError::Database`] when a statement
    /// fails.
    pub(crate) async fn upsert_temporal_catalog_batch_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        upserts: &[TemporalCatalogUpsert],
        recorded_at: DateTime<Utc>,
    ) -> Result<TemporalCatalog, TemporalStoreError> {
        let evolutions = self.upsert_temporal_entities_batch(tx, target, upserts, recorded_at).await?;
        let streams = self.resolve_stream_catalog_batch(tx, target, upserts, &evolutions).await?;
        Ok(TemporalCatalog {
            entity_keys: evolutions.into_keys(),
            streams,
        })
    }
}
