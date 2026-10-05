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
    processor_decoding::{decode_attribute_fragment, decode_entity},
    typed_expander::TypedExpander,
    typed_expansion::TypedExpansion,
    written_member::WrittenMember,
};
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{expansion_context::ExpansionContext, processor::JsonLdEngine},
};
use andromeda_ngsi_ld_entity::document::EntityDocument;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::entity::{Entity, EntityPatch};
use andromeda_ngsi_ld_representation::normalization::normalize_attribute_node;
use indexmap::IndexMap;
use iri_rs::IriBuf;
use jsonld::{ExpandedDocument, syntax::Value as JsonLdValue};
use rdfx::BlankIdBuf;
use serde::Serialize;
use serde_json::{Map, Value};
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
            TypedExpansion::Declined(_) => self.processor_entity(serialized(entity, normalization)?).await,
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
            TypedExpansion::Declined(_) => self.processor_entity(serialized(patch, normalization)?).await,
        }
    }

    /// Expand an entity fragment held as JSON, after its concise rewrite.
    ///
    /// # Errors
    ///
    /// As [`EntityExpansion::entity`].
    pub async fn entity_fragment(&self, mut fragment: Value, normalization: Normalization) -> Result<EntityDocument, ExpansionError> {
        normalization.apply(&mut fragment);
        match TypedExpander::new(&self.context).normalized_fragment(&fragment)? {
            TypedExpansion::Expanded(document) => Ok(document),
            TypedExpansion::Declined(_) => self.processor_entity(fragment).await,
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
            // The member serializes normalized but for the NGSI-LD Null in
            // place of the whole member, which the entity rewrite reads as the
            // concise Property the walk writes for it.
            TypedExpansion::Declined(_) => {
                let document = serialized(&IndexMap::from([(attribute.as_str(), member)]), Normalization::Entity)?;
                Ok(decode_attribute_fragment(attribute, self.processor(document).await?)?)
            }
        }
    }

    /// Expand the attribute fragment of a temporal write held as JSON, written
    /// for the attribute the request path names, after its concise rewrite.
    ///
    /// # Errors
    ///
    /// As [`EntityExpansion::entity`].
    pub async fn attribute_fragment(&self, attribute: &AttributeName, mut fragment: Value) -> Result<AttributeFragment, ExpansionError> {
        normalize_attribute_node(attribute.as_str(), &mut fragment);
        match TypedExpander::new(&self.context).normalized_attribute(attribute, &fragment)? {
            TypedExpansion::Expanded(expanded) => Ok(expanded),
            TypedExpansion::Declined(_) => {
                let document = Value::Object(Map::from_iter([(attribute.to_string(), fragment)]));
                Ok(decode_attribute_fragment(attribute, self.processor(document).await?)?)
            }
        }
    }

    /// Expand an entity document with the processor and read it.
    async fn processor_entity(&self, document: Value) -> Result<EntityDocument, ExpansionError> {
        Ok(decode_entity(self.processor(document).await?)?)
    }

    /// Expand a compact document with the processor.
    async fn processor(&self, document: Value) -> Result<ExpandedDocument<IriBuf, BlankIdBuf>, ExpansionError> {
        self.context
            .expand(self.engine, &JsonLdValue::from_serde_json(document))
            .await
            .map_err(ExpansionError::Processor)
    }
}

/// The JSON a typed compact request serializes to, after its concise rewrite:
/// what the processor expands.
fn serialized<T: Serialize>(request: &T, normalization: Normalization) -> Result<Value, ExpansionError> {
    let mut json = serde_json::to_value(request).map_err(ExpansionError::CompactDocument)?;
    normalization.apply(&mut json);
    Ok(json)
}
