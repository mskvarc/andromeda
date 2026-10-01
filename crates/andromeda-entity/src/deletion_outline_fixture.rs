//! A deletion-outline reader for tests, answering from a fixture payload.

use crate::{
    deletion_outline::{DeletionOutline, DeletionWatchers},
    error::EntityError,
    expanded_member::stored_entity_types,
    port::deletion_outline::DeletionOutlineReader,
};
use andromeda_context_commit::scope_presence::ScopePresence;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::expanded::{instance_key::attribute_instance_families, scope_member::extract_scopes};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// What the deletion-watcher probe answers.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ProbeAnswer {
    /// The probe answers this.
    Answers(DeletionWatchers),
    /// The probe fails as a store outage would.
    Fails,
}

/// Outlines the one fixture payload and answers the probe as configured.
pub(crate) struct PayloadOutline {
    /// The payload the outline is read from.
    pub(crate) payload: Value,
    /// The version the payload is outlined at.
    pub(crate) modified_at: DateTime<Utc>,
    /// What the watcher probe answers.
    pub(crate) probe: ProbeAnswer,
}

/// The outline a store keeps for `payload`, stored at `modified_at`.
pub(crate) fn outline_of(payload: &Value, modified_at: DateTime<Utc>) -> DeletionOutline {
    let mut instances: Vec<_> = attribute_instance_families(payload)
        .expect("the fixture's dataset identifiers read")
        .into_iter()
        .collect();
    instances.sort_by(|(left, _), (right, _)| left.attr.as_str().cmp(right.attr.as_str()));
    DeletionOutline {
        types: stored_entity_types(payload).expect("the fixture carries its types"),
        instances,
        scope: if extract_scopes(payload).is_empty() {
            ScopePresence::Absent
        } else {
            ScopePresence::Held
        },
        modified_at,
    }
}

#[async_trait]
impl DeletionOutlineReader for PayloadOutline {
    async fn deletion_outline(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<DeletionOutline>, EntityError> {
        Ok(Some(outline_of(&self.payload, self.modified_at)))
    }

    async fn deletion_watchers(&self, _target: &ContextTarget) -> Result<DeletionWatchers, EntityError> {
        match self.probe {
            ProbeAnswer::Answers(watchers) => Ok(watchers),
            ProbeAnswer::Fails => Err(EntityError::StoreSaturated),
        }
    }
}
