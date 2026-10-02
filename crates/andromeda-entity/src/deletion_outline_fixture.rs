//! A deletion-outline reader for tests, answering from a fixture state.

use crate::{
    deletion_outline::{DeletionOutline, DeletionWatchers},
    error::EntityError,
    port::deletion_outline::DeletionOutlineReader,
};
use andromeda_context_commit::scope_presence::ScopePresence;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::{entity_state::EntityState, reads::attribute_instance_key::AttributeInstanceKey};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// What the deletion-watcher probe answers.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ProbeAnswer {
    /// The probe answers this.
    Answers(DeletionWatchers),
    /// The probe fails as a store outage would.
    Fails,
}

/// Outlines the one fixture state and answers the probe as configured.
pub(crate) struct PayloadOutline {
    /// The state the outline is read from.
    pub(crate) state: Arc<EntityState>,
    /// The version the state is outlined at.
    pub(crate) modified_at: DateTime<Utc>,
    /// What the watcher probe answers.
    pub(crate) probe: ProbeAnswer,
}

/// The outline a store keeps for `state`, stored at `modified_at`.
pub(crate) fn outline_of(state: &EntityState, modified_at: DateTime<Utc>) -> DeletionOutline {
    let mut instances: Vec<_> = state
        .instance_families()
        .map(|(attr, dataset_id, family)| {
            (
                AttributeInstanceKey {
                    attr: attr.clone(),              // clone: the outline owns the identities it lists
                    dataset_id: dataset_id.cloned(), // clone: the outline owns the identities it lists
                },
                family,
            )
        })
        .collect();
    instances.sort_by(|(left, _), (right, _)| left.attr.as_str().cmp(right.attr.as_str()));
    DeletionOutline {
        types: state.types().as_slice().to_vec(),
        instances,
        scope: if state.scope().is_some() {
            ScopePresence::Held
        } else {
            ScopePresence::Absent
        },
        modified_at,
    }
}

#[async_trait]
impl DeletionOutlineReader for PayloadOutline {
    async fn deletion_outline(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<DeletionOutline>, EntityError> {
        Ok(Some(outline_of(&self.state, self.modified_at)))
    }

    async fn deletion_watchers(&self, _target: &ContextTarget) -> Result<DeletionWatchers, EntityError> {
        match self.probe {
            ProbeAnswer::Answers(watchers) => Ok(watchers),
            ProbeAnswer::Fails => Err(EntityError::StoreSaturated),
        }
    }
}
