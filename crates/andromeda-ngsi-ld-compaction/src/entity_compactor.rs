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
    name_compaction::NameCompaction,
    patch_reading::PatchReading,
    processor_fallback::{
        document_entity_through_processor,
        document_patch_through_processor,
        entity_through_processor,
        scope_history_through_processor,
        temporal_through_processor,
    },
    state_reading::{WalkError, walk_entity},
    term_context::TermContext,
    walk::Walk,
};
use andromeda_jsonld::engine::context_compactor::ContextCompactor;
use andromeda_ngsi_ld_entity::{document::EntityDocument, entity_state::EntityState};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    attribute::member_reading::MemberReading,
    entity::{Entity, EntityPatch},
    node_refusal::NodeReadError,
    temporal::{temporal_attribute::TemporalAttribute, temporal_document::TemporalDocument, temporal_entity::TemporalEntity},
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use chrono::{DateTime, Utc};
use std::{str::FromStr, sync::OnceLock};

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
    /// The names the operation compacts outside an entity, built on first use.
    names: OnceLock<NameCompaction>,
}

impl<'a> EntityCompactor<'a> {
    /// A compactor for the entities of the operation `compactor` serves.
    #[must_use]
    pub const fn new(compactor: &'a ContextCompactor<'a>) -> EntityCompactor<'a> {
        EntityCompactor {
            compactor,
            terms: OnceLock::new(),
            names: OnceLock::new(),
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

    /// The compact entity a written document is under the request context,
    /// a member the document removes written as the NGSI-LD Null (clause
    /// 5.8.6 notifies a deleted attribute that way).
    ///
    /// # Errors
    ///
    /// Returns [`CompactionError::JsonLd`] when the request context does not
    /// resolve or process or the processor refuses the document,
    /// [`CompactionError::Encode`] when the document cannot be written for the
    /// processor, and [`CompactionError::Refused`] when the compact model
    /// refuses the compacted entity.
    pub async fn document_entity(&self, document: &EntityDocument) -> Result<Entity, CompactionError> {
        let terms = self.terms().await?;
        let walked = Walk::new(terms).compact_document(document).map(Compacted::Object);
        match walked {
            Ok(tree) => <Entity>::from_node(terms.read(&tree)).map_err(|error| match error {
                NodeReadError::Refused(refused) => CompactionError::Refused(refused),
                NodeReadError::Unreadable(unwritten) => CompactionError::Encode(unwritten),
            }),
            Err(reason) => {
                tracing::debug!(%reason, "compacting the document through the processor");
                document_entity_through_processor(self.compactor, terms, document).await
            }
        }
    }

    /// The compact partial entity a written document is under the request
    /// context: the entity of [`EntityCompactor::document_entity`] with `id`
    /// and `type` optional, which is what a fragment forwarded to a source is
    /// (clauses 5.6.2, 5.6.3 and 5.6.17). A member the document removes is
    /// written as the NGSI-LD Null.
    ///
    /// # Errors
    ///
    /// As [`EntityCompactor::document_entity`].
    pub async fn patch(&self, document: &EntityDocument) -> Result<EntityPatch, CompactionError> {
        self.read_patch(document, PatchReading::Members).await
    }

    /// The compact partial entity a written document is under the request
    /// context, every attribute read as the bare Attribute Fragment of clauses
    /// 5.6.4 and 5.6.19 it compacts to: an instance that names neither a
    /// `type` nor a value member stays the instance it updates.
    ///
    /// # Errors
    ///
    /// As [`EntityCompactor::document_entity`].
    pub async fn attribute_fragments<M: MemberReading>(&self, document: &EntityDocument) -> Result<EntityPatch<M>, CompactionError> {
        self.read_patch(document, PatchReading::Fragments).await
    }

    /// The compact partial entity a written document is, its attributes read
    /// as `reading` says.
    async fn read_patch<M: MemberReading>(&self, document: &EntityDocument, reading: PatchReading) -> Result<EntityPatch<M>, CompactionError> {
        let terms = self.terms().await?;
        let walked = Walk::new(terms).compact_document(document).map(Compacted::Object);
        match walked {
            Ok(tree) => reading.read(terms.read(&tree)).map_err(|error| match error {
                NodeReadError::Refused(refused) => CompactionError::Refused(refused),
                NodeReadError::Unreadable(unwritten) => CompactionError::Encode(unwritten),
            }),
            Err(reason) => {
                tracing::debug!(%reason, "compacting the fragment through the processor");
                document_patch_through_processor(self.compactor, terms, document, reading).await
            }
        }
    }

    /// The compact Temporal Evolution of an entity a document holds under the
    /// request context: every attribute the sequence of its instances, the
    /// Scope history the attribute `scope`, and every attribute a clause 6.3.10
    /// page cut left without an instance named by the term of its expanded
    /// name, with the family its instances recorded.
    ///
    /// # Errors
    ///
    /// Returns [`CompactionError::JsonLd`] when the request context does not
    /// resolve or process or the processor refuses the document,
    /// [`CompactionError::Encode`] when the document cannot be written for the
    /// processor, and [`CompactionError::Refused`] when the compact model
    /// refuses the compacted entity.
    pub async fn temporal(&self, document: &TemporalDocument) -> Result<TemporalEntity, CompactionError> {
        let terms = self.terms().await?;
        let walked = Walk::new(terms).compact_document(document.entity()).map(Compacted::Object);
        let mut entity = match walked {
            Ok(tree) => TemporalEntity::from_node(terms.read(&tree)).map_err(|error| match error {
                NodeReadError::Refused(refused) => CompactionError::Refused(refused),
                NodeReadError::Unreadable(unwritten) => CompactionError::Encode(unwritten),
            })?,
            Err(reason) => {
                tracing::debug!(%reason, "compacting the temporal document through the processor");
                temporal_through_processor(self.compactor, terms, document.entity()).await?
            }
        };
        entity.system_times = document.system_times();
        let names = self.names().await?;
        for (expanded, family) in document.emptied() {
            let compacted = names.compact_attr_iri(expanded.as_str());
            // A term the context gives the name that is no attribute name
            // leaves the name expanded: it is the name the instances were
            // compacted under, or none was.
            let name = AttributeName::from_str(&compacted).unwrap_or_else(|_| expanded.clone()); // clone: the compact name is the expanded one verbatim
            if name.as_str() == compact_properties::SCOPE {
                entity.scope.get_or_insert_with(|| TemporalAttribute::emptied(*family));
            } else {
                entity.attrs.entry(name).or_insert_with(|| TemporalAttribute::emptied(*family));
            }
        }
        Ok(entity)
    }

    /// The reified Scope history (clause 4.5.6) a document holds under the Scope
    /// IRI, compacted under the request context to the instances a source reads
    /// as its `scope`; `None` when the document holds none. The document names
    /// no entity, which a Scope history alone does not.
    ///
    /// # Errors
    ///
    /// As [`EntityCompactor::temporal`].
    pub async fn scope_history(&self, document: &EntityDocument) -> Result<Option<TemporalAttribute>, CompactionError> {
        let terms = self.terms().await?;
        let walked = Walk::new(terms).compact_document(document).map(Compacted::Object);
        match walked {
            Ok(tree) => TemporalAttribute::scope_history_of(terms.read(&tree)).map_err(|error| match error {
                NodeReadError::Refused(refused) => CompactionError::Refused(refused),
                NodeReadError::Unreadable(unwritten) => CompactionError::Encode(unwritten),
            }),
            Err(reason) => {
                tracing::debug!(%reason, "compacting the scope history through the processor");
                scope_history_through_processor(self.compactor, terms, document).await
            }
        }
    }

    /// The names of the operation, compacted against the request context, built
    /// on first use.
    ///
    /// # Errors
    ///
    /// Returns [`CompactionError::JsonLd`] when the request context does not
    /// resolve or process.
    pub async fn names(&self) -> Result<&NameCompaction, CompactionError> {
        if let Some(names) = self.names.get() {
            return Ok(names);
        }
        let context = self.compactor.processed_context().await?;
        Ok(self.names.get_or_init(|| NameCompaction::new(context)))
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
