//! Expanding a request without the JSON-LD processor.
//!
//! The processed `@context` of the request decides every term (clause 5.5.7);
//! the walk only replaces the processor's generic document tree with the
//! model the request is read into. What it does not model it declines, and the
//! caller hands the request to the processor ([`crate::entity_expansion`]).

use crate::{
    assembly::{attribute_fragment, entity_document},
    attribute_fragment::AttributeFragment,
    compact_walk::{
        element::{Expanded, expand_element},
        member::expand_member,
        object::property_name,
    },
    decline::Decline,
    expanded_tree::{ExpandedItem, ExpandedNode},
    normalization::Normalization,
    term_scope::TermScope,
    typed_expansion::TypedExpansion,
    typed_walk::{entity_node, patch_node},
};
use andromeda_jsonld::engine::expansion_context::ExpansionContext;
use andromeda_ngsi_ld_entity::{document::EntityDocument, refusal::EntityRefusal};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::entity::{Entity, EntityPatch};
use jsonld::Term;
use serde_json::Value;

/// Expands requests under one processed request context.
#[derive(Clone, Copy, Debug)]
pub struct TypedExpander<'c> {
    /// The request's processed context.
    context: &'c ExpansionContext,
}

impl<'c> TypedExpander<'c> {
    /// An expander deciding terms against `context`.
    #[must_use]
    pub const fn new(context: &'c ExpansionContext) -> TypedExpander<'c> {
        TypedExpander { context }
    }

    /// Expand an entity the transport decoded into the typed compact model
    /// (create, upsert, batch), after the concise rewrite `normalization`
    /// names.
    ///
    /// # Errors
    ///
    /// Returns the [`EntityRefusal`] of an expanded document that is not a
    /// write.
    pub fn entity(&self, entity: &Entity, normalization: Normalization) -> Result<TypedExpansion<EntityDocument>, EntityRefusal> {
        self.document(|scope| entity_node(scope, entity, normalization))
    }

    /// Expand an entity fragment the transport decoded into the typed compact
    /// model (merge, replace).
    ///
    /// # Errors
    ///
    /// As [`TypedExpander::entity`].
    pub fn entity_patch(&self, patch: &EntityPatch, normalization: Normalization) -> Result<TypedExpansion<EntityDocument>, EntityRefusal> {
        self.document(|scope| patch_node(scope, patch, normalization))
    }

    /// Expand an entity fragment held as JSON (append, update) that already
    /// went through its concise rewrite.
    ///
    /// # Errors
    ///
    /// As [`TypedExpander::entity`].
    pub fn normalized_fragment(&self, fragment: &Value) -> Result<TypedExpansion<EntityDocument>, EntityRefusal> {
        if !fragment.is_object() {
            return Ok(TypedExpansion::Declined(Decline::MalformedJsonLd));
        }
        self.document(|scope| expand_element(scope, None, fragment))
    }

    /// Expand the attribute fragment of a Partial Attribute Update or a
    /// Replace Attribute (clauses 5.6.4, 5.6.19) that already went through its
    /// concise rewrite, written for the attribute the request path names.
    ///
    /// # Errors
    ///
    /// Returns the [`EntityRefusal`] of a fragment that is not one instance.
    pub fn normalized_attribute(&self, attribute: &AttributeName, fragment: &Value) -> Result<TypedExpansion<AttributeFragment>, EntityRefusal> {
        let walked = TermScope::root(self.context).and_then(|scope| {
            let key = attribute.as_str();
            let term = scope.expand_key(key)?;
            let Term::Id(property) = term.as_ref() else {
                return Err(Decline::MalformedJsonLd);
            };
            let name = property_name(property).ok_or(Decline::MalformedJsonLd)?;
            Ok((name, expand_member(&scope, key, fragment)?.unwrap_or_default()))
        });
        match walked {
            Ok((name, items)) => attribute_fragment(&name, items).map(TypedExpansion::Expanded),
            Err(decline) => Ok(TypedExpansion::Declined(decline)),
        }
    }

    /// Walk a document with `walk` and read the entity it expanded to.
    fn document(self, walk: impl FnOnce(&TermScope<'c>) -> Result<Expanded, Decline>) -> Result<TypedExpansion<EntityDocument>, EntityRefusal> {
        let walked = TermScope::root(self.context).and_then(|scope| walk(&scope)).and_then(entity_node_of);
        match walked {
            Ok(node) => entity_document(node).map(TypedExpansion::Expanded),
            Err(decline) => Ok(TypedExpansion::Declined(decline)),
        }
    }
}

/// The entity node a top-level document expanded to, or none for a document
/// JSON-LD drops whole.
fn entity_node_of(expanded: Expanded) -> Result<Option<ExpandedNode>, Decline> {
    match expanded {
        Expanded::Null => Ok(None),
        Expanded::One(ExpandedItem::Node(node)) => Ok(Some(node)),
        Expanded::One(ExpandedItem::Value(_) | ExpandedItem::Json(_) | ExpandedItem::List(_) | ExpandedItem::Geometry(_)) | Expanded::Many(_) => {
            Err(Decline::MalformedJsonLd)
        }
    }
}
