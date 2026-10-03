//! What one write observed of an entity, as the residual entity selection of a
//! temporal query reads it.
//!
//! A deferred sink accepts a write as an intent and applies it later, so a
//! query answered in between decides whether the intent's entity belongs to the
//! result from what the write itself observed: the instances a Core write
//! recorded (clause 4.3.1), or the instances a Temporal API write added
//! (clauses 5.6.11 and 5.6.12). The intent shares the write's own observation
//! rather than copying it, and persists it as the expanded document it is.

use andromeda_entity::{query_residual::ResidualFilter, residual_filter};
use andromeda_ngsi_ld_entity::{document::EntityDocument, entity_state::EntityState};
use andromeda_ngsi_ld_query::split_filter::{LeafTruths, SplitFilter};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::Arc;

/// The entity one write observed.
#[derive(Clone, Debug, PartialEq)]
pub enum ObservedSelection {
    /// The instances a Core write recorded, as the entity state they stand in.
    Observed(Arc<EntityState>),
    /// The instances a Temporal API write added, several per `datasetId`
    /// possibly, as the document the client wrote.
    Written(Arc<EntityDocument>),
}

impl ObservedSelection {
    /// Whether the observed entity satisfies every predicate of `residual`.
    #[must_use]
    pub fn matches(&self, residual: &ResidualFilter) -> bool {
        match self {
            ObservedSelection::Observed(state) => residual_filter::matches(residual, state),
            ObservedSelection::Written(document) => residual_filter::matches_document(residual, document),
        }
    }

    /// The truth of every leaf of `filter` over the observed entity, which is
    /// the part of it this write contributes (clause 5.7.4.4).
    #[must_use]
    pub fn leaf_truths(&self, filter: &SplitFilter<'_>) -> LeafTruths {
        match self {
            ObservedSelection::Observed(state) => filter.truths_of(state.as_ref()),
            ObservedSelection::Written(document) => filter.truths_of(document.as_ref()),
        }
    }
}

/// Written by hand because the two variants persist as one form: the expanded
/// document each of them is, which a derive would wrap in a variant tag.
impl Serialize for ObservedSelection {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            ObservedSelection::Observed(state) => state.serialize(serializer),
            ObservedSelection::Written(document) => document.serialize(serializer),
        }
    }
}

/// Written by hand for the reason [`Serialize`] is: the persisted form is the
/// expanded document alone, which reads back as the written document it is
/// whichever write observed it.
impl<'de> Deserialize<'de> for ObservedSelection {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<ObservedSelection, D::Error> {
        EntityDocument::deserialize(deserializer).map(|document| ObservedSelection::Written(Arc::new(document)))
    }
}
