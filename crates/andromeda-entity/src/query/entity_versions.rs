//! The entities a read holds itself as versions to merge with what context
//! sources answered, and the merged versions compacted for the response
//! (clauses 4.5.5, 5.7.1.4, 5.7.2.4).
//!
//! A version is an entity in the model beside its entity-level clause 4.8
//! system times. The local store's and every source's versions of one entity
//! merge on the model, by expanded name, and the result is compacted once
//! under the request context: what the response writes is decided after the
//! merge, the same way for every holder.

use crate::{
    error::EntityError,
    port::retrieved_entity::RetrievedEntity,
    query::{EntityQueryService, dataset_filter::filter_entity_by_dataset_id},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_compaction::entity_compactor::{EntityCompactor, EntityRead};
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{entity::Entity, entity_system_times::EntitySystemTimes};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use chrono::Utc;
use std::sync::Arc;

/// How merged versions are written for a response.
#[derive(Clone, Copy, Debug)]
pub struct VersionRead<'r> {
    /// The identifier each version is named by, when the read was keyed by
    /// one (a retrieval).
    pub id_override: Option<&'r Identifier>,
    /// Whether the clause 4.8 system times are written.
    pub sys_attrs: SysAttrsInclusion,
    /// The attribute instances the response is narrowed to; empty selects all
    /// (clause 4.5.5).
    pub dataset_ids: &'r [DatasetSelector],
}

/// A locally held entity as the version the merge builds on.
#[must_use]
pub fn held_version(retrieved: RetrievedEntity) -> ReceivedState {
    let RetrievedEntity { state, sys_attrs } = retrieved;
    ReceivedState {
        // clone: the store shares a decoded state with its cache, the merge
        // folds other versions into its own copy
        state: Arc::unwrap_or_clone(state),
        system_times: EntitySystemTimes {
            created_at: Some(sys_attrs.created_at),
            modified_at: Some(sys_attrs.modified_at),
            deleted_at: None,
        },
        linked: Vec::new(),
    }
}

impl EntityQueryService {
    /// The compact entities `versions` are under `context`, each written as
    /// `read` asks.
    ///
    /// # Errors
    ///
    /// Returns the failure compaction reports for a version that does not
    /// compact under `context`.
    pub async fn compact_versions(&self, versions: &[ReceivedState], read: &VersionRead<'_>, context: &ContextSpec) -> Result<Vec<Entity>, EntityError> {
        let compactor = self.engine().compactor(context);
        let entity_compactor = EntityCompactor::new(&compactor);
        let entity_read = EntityRead {
            id_override: read.id_override,
            sys_attrs: read.sys_attrs,
            now: Utc::now(),
        };
        let mut entities = Vec::with_capacity(versions.len());
        for version in versions {
            let mut entity = entity_compactor.entity(&version.state, &entity_read).await?;
            // Clause 4.8: the entity-level system times are the version's own,
            // read beside the state rather than held in it.
            if read.sys_attrs == SysAttrsInclusion::Include {
                version.system_times.restore(&mut entity);
            }
            if !read.dataset_ids.is_empty() {
                filter_entity_by_dataset_id(&mut entity, read.dataset_ids);
            }
            entities.push(entity);
        }
        Ok(entities)
    }
}
