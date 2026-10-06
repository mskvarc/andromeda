//! Expanding a write request: the typed walk first, the JSON-LD processor for
//! what the walk declines (design section 4.2).
//!
//! Both paths read the request under the same processed context and end in
//! the same assembly; the processor starts from that context rather than
//! processing the request's `@context` once more.

use crate::{
    attribute_fragment::AttributeFragment,
    error::ExpansionError,
    normalization::Normalization,
    processor_decoding::{decode_attribute_fragment, decode_entity, decode_temporal},
    processor_request::{attribute_text, entity_text, patch_text},
    typed_expander::TypedExpander,
    typed_expansion::TypedExpansion,
    written_member::WrittenMember,
};
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{expansion_context::ExpansionContext, processor::JsonLdEngine},
};
use andromeda_ngsi_ld_entity::{document::EntityDocument, refusal::EntityRefusal};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::{
    entity::{Entity, EntityPatch},
    temporal::{temporal_document::TemporalDocument, temporal_entity::TemporalEntity},
};
use iri_rs::IriBuf;
use jsonld::ExpandedDocument;
use rdfx::BlankIdBuf;
use std::sync::Arc;

/// Expands the write requests of one operation under its request context.
#[derive(Clone, Debug)]
pub struct EntityExpansion<'e> {
    /// The engine the processor resolves contexts through.
    engine: &'e JsonLdEngine,
    /// The request's processed context.
    context: Arc<ExpansionContext>,
}

impl<'e> EntityExpansion<'e> {
    /// Process the context `spec` selects.
    ///
    /// # Errors
    ///
    /// Returns [`ExpansionError::Context`] for a context that does not
    /// resolve or does not process.
    pub async fn prepare(engine: &'e JsonLdEngine, spec: &ContextSpec) -> Result<EntityExpansion<'e>, ExpansionError> {
        let context = engine.expansion_context(spec).await.map_err(ExpansionError::Context)?;
        Ok(EntityExpansion::from_context(engine, context))
    }

    /// Expand under a context already processed, so requests sharing one
    /// `@context` are expanded without processing it again.
    #[must_use]
    pub const fn from_context(engine: &'e JsonLdEngine, context: Arc<ExpansionContext>) -> EntityExpansion<'e> {
        EntityExpansion { engine, context }
    }

    /// The processed context requests are expanded under.
    #[must_use]
    pub fn context(&self) -> &ExpansionContext {
        &self.context
    }

    /// Expand an entity decoded into the typed compact model.
    ///
    /// # Errors
    ///
    /// Returns [`ExpansionError::Refused`] for an expanded document that is no
    /// write, and the processor's failure for a request it expanded.
    pub async fn entity<M: WrittenMember>(&self, entity: &Entity<M>, normalization: Normalization) -> Result<EntityDocument, ExpansionError> {
        match TypedExpander::new(&self.context).entity(entity, normalization)? {
            TypedExpansion::Expanded(document) => Ok(document),
            TypedExpansion::Declined(_) => self.processor_entity(&entity_text(entity, normalization)?).await,
        }
    }

    /// Expand an entity fragment decoded into the typed compact model.
    ///
    /// # Errors
    ///
    /// As [`EntityExpansion::entity`].
    pub async fn entity_patch<M: WrittenMember>(&self, patch: &EntityPatch<M>, normalization: Normalization) -> Result<EntityDocument, ExpansionError> {
        match TypedExpander::new(&self.context).entity_patch(patch, normalization)? {
            TypedExpansion::Expanded(document) => Ok(document),
            TypedExpansion::Declined(_) => self.processor_entity(&patch_text(patch, normalization)?).await,
        }
    }

    /// Expand the attribute fragment of clauses 5.6.4 and 5.6.19 the transport
    /// decoded into the typed compact model, written for the attribute the
    /// request path names.
    ///
    /// # Errors
    ///
    /// As [`EntityExpansion::entity`].
    pub async fn attribute<M: WrittenMember>(&self, attribute: &AttributeName, member: &M) -> Result<AttributeFragment, ExpansionError> {
        match TypedExpander::new(&self.context).attribute(attribute, member)? {
            TypedExpansion::Expanded(expanded) => Ok(expanded),
            TypedExpansion::Declined(_) => {
                let text = attribute_text(attribute, member)?;
                Ok(decode_attribute_fragment(attribute, self.processor(&text).await?)?)
            }
        }
    }

    /// Expand the Temporal Evolution of an entity a peer answered with, read
    /// into the compact model, into the context-independent document the
    /// histories of every holder are gathered in.
    ///
    /// # Errors
    ///
    /// As [`EntityExpansion::entity`].
    pub async fn temporal(&self, entity: TemporalEntity) -> Result<TemporalDocument, ExpansionError> {
        let (entity, system_times) = entity
            .into_entity()
            .map_err(|source| ExpansionError::Refused(EntityRefusal::ExpandedAttributeName(source)))?;
        match TypedExpander::new(&self.context).temporal(&entity, system_times)? {
            TypedExpansion::Expanded(document) => Ok(document),
            TypedExpansion::Declined(_) => {
                let text = entity_text(&entity, Normalization::Entity)?;
                Ok(decode_temporal(self.processor(&text).await?, system_times)?)
            }
        }
    }

    /// Expand an entity document with the processor and read it.
    async fn processor_entity(&self, text: &[u8]) -> Result<EntityDocument, ExpansionError> {
        Ok(decode_entity(self.processor(text).await?)?)
    }

    /// Expand a compact document with the processor, handed to it as the
    /// JSON text it is written to.
    async fn processor(&self, text: &[u8]) -> Result<ExpandedDocument<IriBuf, BlankIdBuf>, ExpansionError> {
        self.context.expand_text(self.engine, text).await.map_err(ExpansionError::Processor)
    }
}
