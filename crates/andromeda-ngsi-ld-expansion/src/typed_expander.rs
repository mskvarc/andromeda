//! Expanding a request without the JSON-LD processor.
//!
//! The processed `@context` of the request decides every term (clause 5.5.7);
//! the walk only replaces the processor's generic document tree with the
//! model the request is read into. What it does not model it declines, and the
//! caller hands the request to the processor ([`crate::entity_expansion`]).
//! An expander bounded by the entity value limit counts the values it expands
//! and refuses the request once they pass it ([`crate::value_count`]).

use crate::{
    assembly::{attribute_fragment, entity_document, temporal_document::temporal_document},
    attribute_fragment::AttributeFragment,
    compact_walk::element::Expanded,
    decline::Decline,
    error::ExpansionError,
    expanded_tree::{ExpandedItem, ExpandedNode},
    normalization::Normalization,
    prefixed_names,
    term_scope::TermScope,
    typed_expansion::TypedExpansion,
    typed_walk::{entity_node, instance::expanded, patch_node},
    written_member::WrittenMember,
};
use andromeda_jsonld::engine::expansion_context::ExpansionContext;
use andromeda_ngsi_ld_entity::{document::EntityDocument, refusal::EntityRefusal};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::{
    attribute::instance::AttributeValue,
    entity::{Entity, EntityPatch},
    entity_system_times::EntitySystemTimes,
    temporal::temporal_document::TemporalDocument,
};
use andromeda_ngsi_ld_number::json_value::value_census::{JsonValueLimit, ValueTally};
use andromeda_ngsi_ld_representation::payload_budget::PayloadBudgetExceeded;

/// Expands requests under one processed request context.
#[derive(Clone, Copy, Debug)]
pub struct TypedExpander<'c> {
    /// The request's processed context.
    context: &'c ExpansionContext,
    /// The most values a document may expand to below its value carriers;
    /// `None` counts nothing.
    value_limit: Option<JsonValueLimit>,
}

impl<'c> TypedExpander<'c> {
    /// An expander deciding terms against `context`, counting nothing.
    #[must_use]
    pub const fn new(context: &'c ExpansionContext) -> TypedExpander<'c> {
        TypedExpander { context, value_limit: None }
    }

    /// The same expander, refusing a document whose Property values and
    /// `valueList` items expand to more than `limit` values.
    #[must_use]
    pub const fn bounded_by(self, limit: JsonValueLimit) -> TypedExpander<'c> {
        TypedExpander {
            context: self.context,
            value_limit: Some(limit),
        }
    }

    /// Expand an entity the transport decoded into the typed compact model
    /// (create, upsert, batch), after the concise rewrite `normalization`
    /// names.
    ///
    /// # Errors
    ///
    /// Returns the [`EntityRefusal`] of a prefixed name that breaks the clause
    /// 4.6.2 grammar after its colon, and of an expanded document that is not
    /// a write, and [`PayloadBudgetExceeded::EntityJsonValues`] once the values
    /// expanded pass the limit the expander is bounded by.
    pub fn entity<M: WrittenMember>(&self, entity: &Entity<M>, normalization: Normalization) -> Result<TypedExpansion<EntityDocument>, ExpansionError> {
        prefixed_names::entity(self.context, entity)?;
        self.document(|scope| entity_node(scope, entity, normalization), entity_document)
    }

    /// Expand an entity fragment the transport decoded into the typed compact
    /// model (merge, replace).
    ///
    /// # Errors
    ///
    /// As [`TypedExpander::entity`].
    pub fn entity_patch<M: WrittenMember>(
        &self,
        patch: &EntityPatch<M>,
        normalization: Normalization,
    ) -> Result<TypedExpansion<EntityDocument>, ExpansionError> {
        prefixed_names::patch(self.context, patch)?;
        self.document(|scope| patch_node(scope, patch, normalization), entity_document)
    }

    /// Expand the Temporal Evolution of an entity, which a peer answered with
    /// and the compact model read into an entity whose Scope history is the
    /// attribute `scope`, `system_times` being the entity's own clause 4.8
    /// times.
    ///
    /// # Errors
    ///
    /// Returns the [`EntityRefusal`] of an expanded document that is not a
    /// write, and the value refusal of [`TypedExpander::entity`].
    pub fn temporal(&self, entity: &Entity<AttributeValue>, system_times: EntitySystemTimes) -> Result<TypedExpansion<TemporalDocument>, ExpansionError> {
        self.document(
            |scope| entity_node(scope, entity, Normalization::Entity),
            |node| temporal_document(node, system_times),
        )
    }

    /// Expand the attribute fragment of a Partial Attribute Update or a
    /// Replace Attribute (clauses 5.6.4, 5.6.19) the transport decoded into
    /// the typed compact model, written for the attribute the request path
    /// names.
    ///
    /// # Errors
    ///
    /// Returns the [`EntityRefusal`] of a prefixed name that breaks the clause
    /// 4.6.2 grammar after its colon, and of a fragment that is not one
    /// instance, and the value refusal of [`TypedExpander::entity`].
    pub fn attribute<M: WrittenMember>(&self, attribute: &AttributeName, member: &M) -> Result<TypedExpansion<AttributeFragment>, ExpansionError> {
        prefixed_names::attribute(self.context, attribute, member)?;
        self.walk(
            |scope| expanded(member, scope, attribute.as_str())?.ok_or(Decline::MalformedJsonLd),
            |(name, items)| attribute_fragment(&name, items),
        )
    }

    /// Walk a document with `walk` and assemble what it expanded to with
    /// `assemble`.
    fn document<T>(
        self,
        walk: impl FnOnce(&TermScope<'_>) -> Result<Expanded, Decline>,
        assemble: impl FnOnce(Option<ExpandedNode>) -> Result<T, EntityRefusal>,
    ) -> Result<TypedExpansion<T>, ExpansionError> {
        self.walk(|scope| walk(scope).and_then(entity_node_of), assemble)
    }

    /// Walk a request with `walk`, its values counted against the limit the
    /// expander is bounded by, and assemble what it expanded to with
    /// `assemble`.
    fn walk<W, T>(
        self,
        walk: impl FnOnce(&TermScope<'_>) -> Result<W, Decline>,
        assemble: impl FnOnce(W) -> Result<T, EntityRefusal>,
    ) -> Result<TypedExpansion<T>, ExpansionError> {
        let tally = self.value_limit.map(ValueTally::new);
        let walked = TermScope::root(self.context).and_then(|scope| walk(&scope.counting_into(tally.as_ref())));
        match (walked, self.value_limit) {
            (Ok(walked), _) => Ok(TypedExpansion::Expanded(assemble(walked)?)),
            // Clause 6.3.2: the census of the document would refuse it, so it
            // is refused before the rest of it is expanded.
            (Err(Decline::ValueLimit), Some(limit)) => Err(ExpansionError::PayloadBudget(PayloadBudgetExceeded::EntityJsonValues { limit })),
            (Err(decline), _) => Ok(TypedExpansion::Declined(decline)),
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
