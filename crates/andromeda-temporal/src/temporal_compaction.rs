//! Compacting what one temporal read renders against its request context
//! (clause 5.5.7).
//!
//! One read renders many temporal entities and names many attributes, all
//! against one `@context`: the context is processed once, the first time
//! anything is compacted, and every temporal entity and every name of the read
//! is compacted against it.

use crate::error::TemporalError;
use andromeda_jsonld::engine::context_compactor::ContextCompactor;
use andromeda_ngsi_ld_compaction::{entity_compactor::EntityCompactor, name_compaction::NameCompaction};
use andromeda_ngsi_ld_entity::document::EntityDocument;
use async_lock::OnceCell;
use serde_json::Value;

/// The compaction of one temporal read.
#[derive(Debug)]
pub(crate) struct TemporalCompaction<'a> {
    /// The read's compactor: its request context, processed once.
    compactor: &'a ContextCompactor<'a>,
    /// Compacts the temporal entities the read assembles.
    entities: EntityCompactor<'a>,
    /// Compacts the names the read renders outside an entity, built on first
    /// use.
    names: OnceCell<NameCompaction>,
}

impl<'a> TemporalCompaction<'a> {
    /// The compaction of the read `compactor` serves.
    pub(crate) const fn new(compactor: &'a ContextCompactor<'a>) -> TemporalCompaction<'a> {
        TemporalCompaction {
            compactor,
            entities: EntityCompactor::new(compactor),
            names: OnceCell::new(),
        }
    }

    /// The compact JSON an assembled temporal entity is under the request
    /// context.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::JsonLd`] when the request context does not
    /// resolve or process, and [`TemporalError::Internal`] when the assembled
    /// entity does not compact.
    pub(crate) async fn document(&self, document: &EntityDocument) -> Result<Value, TemporalError> {
        Ok(self.entities.document(document).await?)
    }

    /// The names of the read, compacted against the request context.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::JsonLd`] when the request context does not
    /// resolve or process.
    pub(crate) async fn names(&self) -> Result<&NameCompaction, TemporalError> {
        self.names
            .get_or_try_init(|| async { Ok::<NameCompaction, TemporalError>(NameCompaction::new(self.compactor.processed_context().await?)) })
            .await
    }
}
