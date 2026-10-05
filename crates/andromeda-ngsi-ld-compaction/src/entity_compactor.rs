//! Compacting entities for one request (clause 5.5.7, JSON-LD 1.1 API section
//! 6).
//!
//! [`EntityCompactor`] is built once per operation from the operation's
//! [`ContextCompactor`]: the request context is processed once, the first time
//! an entity needs it, and every entity of the operation is compacted against
//! it by the term walk, the keys each attribute selected being remembered for
//! the next entity. An entity the walk leaves to the processor -- one under a
//! context with scoped contexts, `@nest` or map containers -- is compacted by
//! the processor from its canonical expanded form, so either way the result is
//! what the processor gives.

use crate::{
    compacted::Compacted,
    compaction_error::CompactionError,
    processor_fallback::{document_through_processor, entity_through_processor},
    state_reading::{WalkError, walk_entity},
    term_context::TermContext,
    undecidable::Undecidable,
    walk::{GeometryOutput, Walk},
};
use andromeda_jsonld::engine::context_compactor::ContextCompactor;
use andromeda_ngsi_ld_entity::{document::EntityDocument, entity_state::EntityState};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::OnceLock;

/// How one entity is read: under which identifier, with or without its
/// system times, and at which instant its expired instances are dropped.
#[derive(Clone, Copy, Debug)]
pub struct EntityRead<'r> {
    /// The identifier the entity is named by, when the caller read it by one
    /// other than its own (a batch read keyed by identifier).
    pub id_override: Option<&'r Identifier>,
    /// Whether the clause 4.8 `createdAt` and `modifiedAt` of its attribute
    /// instances are written (Table 5.2.14-1).
    pub sys_attrs: SysAttrsInclusion,
    /// The instant at which an instance whose `expiresAt` has passed is no
    /// longer part of the entity (clause 4.22).
    pub now: DateTime<Utc>,
}

/// Compacts the entities of one operation against its request context.
#[derive(Debug)]
pub struct EntityCompactor<'a> {
    /// The operation's compactor: the request context, processed once, and
    /// the processor the walk hands undecidable entities to.
    compactor: &'a ContextCompactor<'a>,
    /// The processed context and the keys selected in it so far.
    terms: OnceLock<TermContext>,
}

impl<'a> EntityCompactor<'a> {
    /// A compactor for the entities of the operation `compactor` serves.
    #[must_use]
    pub const fn new(compactor: &'a ContextCompactor<'a>) -> EntityCompactor<'a> {
        EntityCompactor {
            compactor,
            terms: OnceLock::new(),
        }
    }

    /// The compact entity `state` is under the request context.
    ///
    /// # Errors
    ///
    /// Returns [`CompactionError::JsonLd`] when the request context does not
    /// resolve or process or the processor refuses the entity, and
    /// [`CompactionError::Refused`] when the compact model refuses the
    /// compacted entity.
    pub async fn entity(&self, state: &EntityState, read: &EntityRead<'_>) -> Result<Entity, CompactionError> {
        let terms = self.terms().await?;
        let mut entity = match walk_entity(terms, state, read.id_override, read.sys_attrs) {
            Ok(entity) => entity,
            Err(WalkError::Refused(refusal)) => return Err(refusal),
            Err(WalkError::Undecidable(reason)) => {
                tracing::debug!(%reason, entity_id = state.id().as_str(), "compacting the entity through the processor");
                entity_through_processor(self.compactor, terms, state, read.id_override, read.sys_attrs).await?
            }
        };
        // Clause 4.22: an instance whose `expiresAt` has passed is not part of
        // the entity, and the sweep removing it from storage runs on an
        // interval; dropping it here keeps a client from seeing it between
        // sweeps.
        entity.remove_expired_attributes(read.now);
        Ok(entity)
    }

    /// The compact JSON a written document is under the request context.
    ///
    /// # Errors
    ///
    /// Returns [`CompactionError::JsonLd`] when the request context does not
    /// resolve or process or the processor refuses the document, and
    /// [`CompactionError::Encode`] when the document cannot be written for the
    /// processor.
    pub async fn document(&self, document: &EntityDocument) -> Result<Value, CompactionError> {
        let terms = self.terms().await?;
        match Walk::new(terms, GeometryOutput::Written)
            .compact_document(document)
            .and_then(|node| serde_json::to_value(Compacted::Object(node)).map_err(|_unwritten| Undecidable::UnrepresentableValue))
        {
            Ok(node) => Ok(node),
            Err(reason) => {
                tracing::debug!(%reason, "compacting the document through the processor");
                document_through_processor(self.compactor, document).await
            }
        }
    }

    /// The processed request context, processed on first use.
    ///
    /// Two entities compacted at once may both find it unset; both then ask
    /// the compactor, which processes the context once for both, and the
    /// first to finish keeps the keys it selects.
    async fn terms(&self) -> Result<&TermContext, CompactionError> {
        if let Some(terms) = self.terms.get() {
            return Ok(terms);
        }
        let context = self.compactor.processed_context().await?;
        Ok(self.terms.get_or_init(|| TermContext::new(context)))
    }
}
